//! Where an upload came from: the disc files the game read, and the EMI entries that hold the
//! words it sent.
//!
//! The game positions the drive with `Setloc` before each read, so the files under those sectors
//! are the files it read. Each EMI file it reads is indexed once: its type-3 entries by the 32×32
//! rectangles they upload (`EMI_UploadSectorImage` sends one rectangle per sector), and its type-0
//! entries that land in the palette arena (`EMI_LoadToRam`), whose words reach VRAM through
//! `GFX_UploadPalette`.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use psx_formats::gfx::texture::{Clut, Depth};
use psx_formats::gfx::vram::{self, Descriptor, RECTANGLE_SIDE};
use psx_machine::cdrom::CommandRecord;
use rebof3_emi::texture::ResidentClutBlock;
use rebof3_emi::{Archive, Entry};

/// The CD-ROM command that sets the seek target (psx-spx "CDROM Controller Command Summary",
/// `Setloc` 02h): three BCD bytes, minute, second and sector.
const SETLOC: u8 = 0x02;
/// Sectors per second of CD time (psx-spx "CDROM Disk Format", 75 sectors a second).
const SECTORS_PER_SECOND: u32 = 75;
/// Seconds per minute of CD time.
const SECONDS_PER_MINUTE: u32 = 60;
/// The two-second lead-in before the first sector of the file system (psx-spx "CDROM Disk
/// Format": 00:02:00 is sector 0).
const LEAD_IN_SECTORS: u32 = 150;
/// Bytes per ISO 9660 sector.
const SECTOR_BYTES: u32 = 2048;
/// The entry type `EMI_LoadToRam` copies to RAM.
const TYPE_RAM: u16 = 0;
/// The extension of an EMI archive.
const ARCHIVE_EXTENSION: &str = "EMI";

/// An entry of a file the game read.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EntryRef {
    /// Index into [`Sources::files`].
    pub file: usize,
    /// The entry's slot in the archive.
    pub slot: usize,
}

/// One EMI file the game read.
pub struct File {
    /// The disc path, `/`-separated (`BIN/BATTLE/BATL_DRA.EMI`).
    pub path: String,
    /// The frame of the latest `Setloc` into the file.
    pub last_read: u64,
    /// The CLUT blocks it loads into the arena.
    banks: Vec<ResidentClutBlock>,
}

/// A rectangle a type-3 entry uploads.
struct Chunk {
    entry: EntryRef,
    words: Box<[u16]>,
}

/// The files the game read and the uploads they hold.
pub struct Sources {
    image: psx_cd::Image,
    /// The disc's files, by first sector.
    disc: Vec<psx_cd::Entry>,
    /// The EMI files read so far, in the order the game first read them.
    pub files: Vec<File>,
    by_path: HashMap<String, usize>,
    /// Type-3 rectangles by position and a hash of their words.
    chunks: HashMap<(u16, u16, u64), Vec<Chunk>>,
    /// `Setloc` records handled so far.
    seen: usize,
}

impl Sources {
    /// The index for the disc in `image`, with no file read yet.
    ///
    /// # Errors
    ///
    /// The disc's error when its file system does not read.
    pub fn new(mut image: psx_cd::Image) -> Result<Self, psx_cd::Error> {
        let mut disc: Vec<psx_cd::Entry> = image.entries()?.into_iter().filter(|entry| !entry.is_directory).collect();
        disc.sort_by_key(|entry| entry.lba);
        Ok(Self { image, disc, files: Vec::new(), by_path: HashMap::new(), chunks: HashMap::new(), seen: 0 })
    }

    /// Reads the `Setloc` commands added to `commands` since the last call, and indexes each EMI
    /// file they point into the first time.
    pub fn follow(&mut self, commands: &[CommandRecord]) {
        let start = self.seen.min(commands.len());
        self.seen = commands.len();
        for record in &commands[start..] {
            if record.command != SETLOC || record.parameters.len() != 3 {
                continue;
            }
            let [minute, second, sector] = [0, 1, 2].map(|at| from_bcd(record.parameters[at]));
            let Some(sector) = ((minute * SECONDS_PER_MINUTE + second) * SECTORS_PER_SECOND + sector).checked_sub(LEAD_IN_SECTORS) else {
                continue;
            };
            let Some(entry) = self.file_at(sector).cloned() else { continue };
            let is_archive = entry.path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case(ARCHIVE_EXTENSION));
            if !is_archive {
                continue;
            }
            let path = entry.path.to_string_lossy().replace('\\', "/");
            let file = match self.by_path.get(&path) {
                Some(&file) => file,
                None => self.index(&entry, path),
            };
            self.files[file].last_read = record.frame;
        }
    }

    /// The disc file that holds `sector`.
    fn file_at(&self, sector: u32) -> Option<&psx_cd::Entry> {
        let after = self.disc.partition_point(|entry| entry.lba <= sector);
        let entry = self.disc[..after].last()?;
        (sector < entry.lba + entry.size.div_ceil(SECTOR_BYTES).max(1)).then_some(entry)
    }

    /// Reads and indexes an EMI file; a file that does not read or parse is kept with no entries.
    fn index(&mut self, entry: &psx_cd::Entry, path: String) -> usize {
        let file = self.files.len();
        let mut banks = Vec::new();
        if let Ok(bytes) = self.image.read_file(entry)
            && let Ok(archive) = Archive::parse(&bytes)
        {
            for (slot, table) in archive.entries().iter().enumerate() {
                let Ok(payload) = archive.payload(&bytes, slot) else { continue };
                let at = EntryRef { file, slot };
                match table.file_type {
                    vram::UPLOAD_TYPE => self.index_upload(at, table, payload),
                    TYPE_RAM => banks.extend(ResidentClutBlock::new(path.clone(), slot, table.ram_ptr, payload)),
                    _ => {}
                }
            }
        }
        self.by_path.insert(path.clone(), file);
        self.files.push(File { path, last_read: 0, banks });
        file
    }

    fn index_upload(&mut self, entry: EntryRef, table: &Entry, payload: &[u8]) {
        let Ok(rectangles) = vram::decode(payload, Descriptor::from_load_argument(table.ram_ptr)) else { return };
        for rectangle in rectangles {
            let key = (rectangle.x as u16, rectangle.y as u16, hash(&rectangle.words));
            self.chunks.entry(key).or_default().push(Chunk { entry, words: rectangle.words.into() });
        }
    }

    /// The entry whose rectangle an upload of `width`×`height` words at (`x`, `y`) sent, or `None`
    /// when no file read holds it. Two files with the same rectangle name the one read last.
    pub fn upload(&self, x: u16, y: u16, width: u16, height: u16, pixels: &[u16]) -> Option<EntryRef> {
        if usize::from(width) != RECTANGLE_SIDE || usize::from(height) != RECTANGLE_SIDE {
            return None;
        }
        self.chunks.get(&(x, y, hash(pixels)))?.iter().filter(|chunk| *chunk.words == *pixels).max_by_key(|chunk| self.files[chunk.entry.file].last_read).map(|chunk| chunk.entry)
    }

    /// The arena bank that holds `words` as the CLUT row at `clut`, or `None` when no file read
    /// loads those words there. Two files that do name the one read last.
    pub fn arena_bank(&self, clut: Clut, depth: Depth, words: &[u16]) -> Option<EntryRef> {
        self.files
            .iter()
            .enumerate()
            .flat_map(|(file, record)| record.banks.iter().map(move |bank| (file, bank)))
            .filter(|(_, bank)| bank.row(clut, depth) == Some(words))
            .max_by_key(|(file, _)| self.files[*file].last_read)
            .map(|(file, bank)| EntryRef { file, slot: bank.block.slot })
    }
}

fn from_bcd(value: u8) -> u32 {
    u32::from(value >> 4) * 10 + u32::from(value & 0xF)
}

fn hash(words: &[u16]) -> u64 {
    let mut hasher = DefaultHasher::new();
    words.hash(&mut hasher);
    hasher.finish()
}
