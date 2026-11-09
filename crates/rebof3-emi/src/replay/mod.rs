//! The VRAM a scene leaves behind: the game's archive loads replayed in order.
//!
//! The model holds the 1024×512 VRAM words, the 16 KB palette arena in RAM and its working copy,
//! and, for every VRAM word, the archive entry that wrote it. A [`Step`] is one thing the game's
//! loaders do:
//!
//! - **Clear.** `SYS_StartFrontend` clears the whole frame buffer with `GFX_ClearRect(0, 0, 1024,
//!   512)` before it streams `FIRST`.
//! - **Load.** `EMI_LoadArchive` streams the entries in table order. A type-3 entry is unpacked by
//!   `EMI_LoadToVram` and uploaded one 2,048-byte sector at a time by `EMI_UploadSectorImage`, a
//!   `LoadImage` of a 32×32 rectangle that is skipped unless `x < 1001` and `y < 501`. A type-0
//!   entry is copied to its RAM address by `EMI_LoadToRam`, so one that lands in the arena
//!   (`0x80033800..0x80037800`) changes the palette RAM but not yet VRAM.
//! - **Reset palette.** `GFX_ResetPalette` zeroes arena rows 28-31 and copies all 32 rows to the
//!   working copy at `0x80037800`.
//! - **Copy palette.** `GFX_CopyPalette(row)` copies one 256-word arena row to the working copy.
//! - **Copy words.** `func_801D1228` (the battle reset) copies the 16 words at arena row 15,
//!   column 240 to the working copy.
//! - **Upload palette.** `GFX_UploadPalette` sends the working copy to VRAM with one `LoadImage`
//!   of `rect(0, 480, 256, 32)` whenever a copy raised `g_GfxPalettePending`.
//!
//! A palette word in VRAM is owned by the entry that wrote that arena word, carried through the
//! working copy. Which steps make up a scene is in [`scenes`].
//!
//! Not modelled: the per-frame transfer queue (`GFX_UpdateTransferQueue`), palette fades and
//! cycling, and the palettes actors load at run time (`GFX_LoadPalette`). They change what a frame
//! shows, not what the archives load.

pub mod scenes;

use std::collections::BTreeMap;

use psx_formats::gfx::texture::{Clut, Depth, texel_indices};
use psx_formats::gfx::vram::{self, Descriptor, VRAM_HEIGHT, VRAM_WIDTH};

use crate::Entry;
use crate::texture::{ARENA_BASE, ARENA_BYTES, ARENA_ROW_WORDS, ARENA_VRAM_ROW};

/// Rows of the palette arena and of its upload rectangle.
pub const ARENA_ROWS: usize = 32;
/// Words in the palette arena.
const ARENA_WORDS: usize = ARENA_ROWS * ARENA_ROW_WORDS as usize;
/// The arena rows `GFX_ResetPalette` zeroes before it copies the arena.
const RESET_CLEARS: std::ops::Range<usize> = 28..32;
/// `EMI_UploadSectorImage` uploads a rectangle only when its left edge is below this column.
const UPLOAD_X_LIMIT: usize = 1001;
/// `EMI_UploadSectorImage` uploads a rectangle only when its top edge is below this row.
const UPLOAD_Y_LIMIT: usize = 501;
/// The entry type `EMI_LoadToRam` handles.
const TYPE_RAM: u16 = 0;
/// The entry types `EMI_LoadCompressed` decodes to RAM.
const TYPES_LZ77: [u16; 2] = [1, 2];
/// "No writer" in an owner map.
const NOBODY: u32 = 0;

/// What wrote a span of VRAM.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Writer {
    /// An archive entry.
    Entry {
        /// The archive's disc path (`BIN/ETC/FIRST.EMI`).
        archive: String,
        /// The entry's slot.
        slot: usize,
    },
    /// Anything else that calls [`Vram::load_image`], named by the caller.
    Named(String),
}

impl Writer {
    /// The writer of entry `slot` of `archive`.
    pub fn entry(archive: impl Into<String>, slot: usize) -> Self {
        Self::Entry { archive: archive.into(), slot }
    }

    /// The archive and slot, for an entry.
    pub fn as_entry(&self) -> Option<(&str, usize)> {
        match self {
            Self::Entry { archive, slot } => Some((archive, *slot)),
            Self::Named(_) => None,
        }
    }
}

impl std::fmt::Display for Writer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Entry { archive, slot } => write!(formatter, "{archive} #{slot}"),
            Self::Named(name) => formatter.write_str(name),
        }
    }
}

/// How a span reached VRAM.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    /// A type-3 upload.
    Upload,
    /// Words of the palette arena, uploaded through the working copy.
    Palette,
    /// A [`Vram::load_image`] call.
    Image,
}

/// One loader action; see the module documentation for the function each one replays.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Step {
    /// `GFX_ClearRect(0, 0, 1024, 512)`.
    Clear,
    /// `EMI_LoadArchive` of the archive at this disc path.
    Load(String),
    /// `GFX_ResetPalette`.
    ResetPalette,
    /// `GFX_CopyPalette(row)`.
    CopyPalette(u8),
    /// The working-copy write of `count` words at arena `row`, `column`.
    CopyWords { row: u8, column: u16, count: u16 },
    /// `GFX_UploadPalette`.
    UploadPalette,
}

/// A rectangle of VRAM one entry wrote.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Region {
    /// Left edge in VRAM words.
    pub x: u16,
    /// Top edge in VRAM rows.
    pub y: u16,
    /// Width in VRAM words.
    pub width: u16,
    /// Height in VRAM rows.
    pub height: u16,
    /// The entry that wrote it.
    pub writer: Writer,
    /// Whether it is an upload or palette words.
    pub kind: Kind,
}

/// One archive as the loader sees it: the table and each entry's payload.
pub struct Archive<'a> {
    /// The table entries.
    pub entries: &'a [Entry],
    /// One payload per entry.
    pub payloads: Vec<&'a [u8]>,
}

/// The frame buffer and palette RAM after a sequence of [`Step`]s.
#[derive(Clone)]
pub struct Vram {
    words: Vec<u16>,
    owners: Vec<u32>,
    kinds: Vec<Kind>,
    writers: Vec<Writer>,
    arena: Vec<u16>,
    arena_owners: Vec<u32>,
    work: Vec<u16>,
    work_owners: Vec<u32>,
    pending: bool,
    missing: Vec<String>,
    skipped: usize,
}

impl Default for Vram {
    fn default() -> Self {
        Self::new()
    }
}

impl Vram {
    /// An all-zero frame buffer and palette RAM that no entry wrote.
    pub fn new() -> Self {
        Self {
            words: vec![0; VRAM_WIDTH * VRAM_HEIGHT],
            owners: vec![NOBODY; VRAM_WIDTH * VRAM_HEIGHT],
            kinds: Vec::new(),
            writers: Vec::new(),
            arena: vec![0; ARENA_WORDS],
            arena_owners: vec![NOBODY; ARENA_WORDS],
            work: vec![0; ARENA_WORDS],
            work_owners: vec![NOBODY; ARENA_WORDS],
            pending: false,
            missing: Vec::new(),
            skipped: 0,
        }
    }

    /// Replays `steps` in order. `disc` returns an archive by disc path; an archive it cannot
    /// give is recorded in [`Vram::missing`] and skipped.
    pub fn replay<'a, D>(steps: &[Step], disc: &mut D) -> Self
    where
        D: FnMut(&str) -> Option<Archive<'a>>,
    {
        let mut vram = Self::new();
        for step in steps {
            vram.apply(step, disc);
        }
        vram
    }

    /// Applies one step.
    pub fn apply<'a, D>(&mut self, step: &Step, disc: &mut D)
    where
        D: FnMut(&str) -> Option<Archive<'a>>,
    {
        match step {
            Step::Clear => self.clear(),
            Step::Load(path) => match disc(path) {
                Some(archive) => self.load(path, archive.entries, &archive.payloads),
                None => self.missing.push(path.clone()),
            },
            Step::ResetPalette => self.reset_palette(),
            Step::CopyPalette(row) => self.copy_palette(*row),
            Step::CopyWords { row, column, count } => self.copy_words(*row, *column, *count),
            Step::UploadPalette => self.upload_palette(),
        }
    }

    /// `GFX_ClearRect(0, 0, 1024, 512)`: every word zero, written by nobody.
    pub fn clear(&mut self) {
        self.words.fill(0);
        self.owners.fill(NOBODY);
    }

    /// Streams one archive: uploads its type-3 entries and copies the arena part of its RAM
    /// entries, in table order.
    pub fn load(&mut self, archive: &str, entries: &[Entry], payloads: &[&[u8]]) {
        for (slot, (entry, payload)) in entries.iter().zip(payloads).enumerate() {
            let writer = Writer::entry(archive, slot);
            match entry.file_type {
                vram::UPLOAD_TYPE => self.upload(writer, entry.ram_ptr, payload),
                TYPE_RAM => self.write_ram(writer, entry.ram_ptr, payload),
                kind if TYPES_LZ77.contains(&kind) => {
                    if overlaps_arena(entry.ram_ptr, payload.len())
                        && let Ok(decoded) = crate::lz77::decode(payload)
                    {
                        self.write_ram(writer, entry.ram_ptr, &decoded);
                    }
                }
                _ => {}
            }
        }
    }

    /// `GFX_ResetPalette`: zero arena rows 28-31, then copy every row to the working copy.
    pub fn reset_palette(&mut self) {
        let row_words = usize::from(ARENA_ROW_WORDS);
        let cleared = RESET_CLEARS.start * row_words..RESET_CLEARS.end * row_words;
        self.arena[cleared.clone()].fill(0);
        self.arena_owners[cleared].fill(NOBODY);
        self.work.copy_from_slice(&self.arena);
        self.work_owners.copy_from_slice(&self.arena_owners);
        self.pending = true;
    }

    /// `GFX_CopyPalette(row)`: copy one arena row to the working copy.
    pub fn copy_palette(&mut self, row: u8) {
        self.copy_words(row, 0, ARENA_ROW_WORDS);
    }

    /// Copies `count` arena words from `row`, `column` to the working copy.
    pub fn copy_words(&mut self, row: u8, column: u16, count: u16) {
        let start = usize::from(row) * usize::from(ARENA_ROW_WORDS) + usize::from(column);
        let end = (start + usize::from(count)).min(ARENA_WORDS);
        if start >= end {
            return;
        }
        self.work[start..end].copy_from_slice(&self.arena[start..end]);
        self.work_owners[start..end].copy_from_slice(&self.arena_owners[start..end]);
        self.pending = true;
    }

    /// `GFX_UploadPalette`: the working copy to `rect(0, 480, 256, 32)` when a copy is pending.
    pub fn upload_palette(&mut self) {
        if !self.pending {
            return;
        }
        let words = std::mem::take(&mut self.work);
        let owners = std::mem::take(&mut self.work_owners);
        self.write_rect(0, usize::from(ARENA_VRAM_ROW), usize::from(ARENA_ROW_WORDS), ARENA_ROWS, &words, &owners);
        self.work = words;
        self.work_owners = owners;
        self.pending = false;
    }

    /// `LoadImage`: writes `width`×`height` words, row-major, at `x`, `y`, clipped to VRAM.
    pub fn load_image(&mut self, x: usize, y: usize, width: usize, height: usize, words: &[u16], writer: Writer) {
        let owner = self.owner(writer, Kind::Image);
        let owners = vec![owner; words.len()];
        self.write_rect(x, y, width, height, words, &owners);
    }

    /// The word at column `x`, row `y`, or 0 outside VRAM.
    pub fn word(&self, x: usize, y: usize) -> u16 {
        if x >= VRAM_WIDTH || y >= VRAM_HEIGHT {
            return 0;
        }
        self.words[y * VRAM_WIDTH + x]
    }

    /// Every VRAM word, row-major, 1024 per row.
    pub fn words(&self) -> &[u16] {
        &self.words
    }

    /// The entry that wrote the word at `x`, `y`, if any.
    pub fn writer(&self, x: usize, y: usize) -> Option<&Writer> {
        if x >= VRAM_WIDTH || y >= VRAM_HEIGHT {
            return None;
        }
        self.writer_of(self.owners[y * VRAM_WIDTH + x])
    }

    /// How the word at `x`, `y` reached VRAM, if anything wrote it.
    pub fn kind(&self, x: usize, y: usize) -> Option<Kind> {
        if x >= VRAM_WIDTH || y >= VRAM_HEIGHT {
            return None;
        }
        let owner = self.owners[y * VRAM_WIDTH + x];
        owner.checked_sub(1).and_then(|index| self.kinds.get(index as usize).copied())
    }

    /// The one entry that wrote every word of the rectangle, or `None` when a word is unwritten,
    /// the rectangle leaves VRAM, or two entries share it.
    pub fn sole_writer(&self, x: usize, y: usize, width: usize, height: usize) -> Option<&Writer> {
        if width == 0 || height == 0 || x + width > VRAM_WIDTH || y + height > VRAM_HEIGHT {
            return None;
        }
        let first = self.owners[y * VRAM_WIDTH + x];
        let uniform = (y..y + height).all(|row| {
            let start = row * VRAM_WIDTH + x;
            self.owners[start..start + width].iter().all(|&owner| owner == first)
        });
        if uniform { self.writer_of(first) } else { None }
    }

    /// The one entry that wrote the whole CLUT row `clut`, `depth.colors()` words long.
    pub fn clut_writer(&self, clut: Clut, depth: Depth) -> Option<&Writer> {
        self.sole_writer(usize::from(clut.x), usize::from(clut.y), depth.colors(), 1)
    }

    /// The `depth.colors()` words of the CLUT row at `clut`, clipped at the right edge.
    pub fn clut_words(&self, clut: Clut, depth: Depth) -> Vec<u16> {
        let start = usize::from(clut.y) * VRAM_WIDTH + usize::from(clut.x);
        let end = (start + depth.colors()).min(start - start % VRAM_WIDTH + VRAM_WIDTH);
        self.words.get(start..end).map(<[u16]>::to_vec).unwrap_or_default()
    }

    /// The texel indices of the `width`×`height` word rectangle at `x`, `y`, read at `depth`,
    /// row-major, `width * depth.texels_per_word()` per row. Words outside VRAM read zero.
    pub fn texels(&self, x: usize, y: usize, width: usize, height: usize, depth: Depth) -> Vec<u8> {
        let mut indices = Vec::with_capacity(width * height * depth.texels_per_word());
        for row in y..y + height {
            let words: Vec<u16> = (x..x + width).map(|column| self.word(column, row)).collect();
            indices.extend(texel_indices(&words, depth));
        }
        indices
    }

    /// The colour word a textured primitive reads at texel `u`, `v` of the page whose top-left
    /// word is `page_x`, `page_y`, through the CLUT at `clut`: the GPU's texture fetch without a
    /// texture window. A 4-bit texel is the `u % 4`-th nibble of word `u / 4`, an 8-bit texel the
    /// `u % 2`-th byte of word `u / 2`, low first. Coordinates wrap at the VRAM edges, as the GPU's
    /// do.
    pub fn fetch(&self, page_x: u16, page_y: u16, clut: Clut, depth: Depth, u: u8, v: u8) -> u16 {
        let per_word = depth.texels_per_word();
        let bits = depth.bits() as usize;
        let x = (usize::from(page_x) + usize::from(u) / per_word) % VRAM_WIDTH;
        let y = (usize::from(page_y) + usize::from(v)) % VRAM_HEIGHT;
        let shift = (usize::from(u) % per_word) * bits;
        let index = (usize::from(self.words[y * VRAM_WIDTH + x]) >> shift) & ((1 << bits) - 1);
        let column = (usize::from(clut.x) + index) % VRAM_WIDTH;
        self.words[usize::from(clut.y) % VRAM_HEIGHT * VRAM_WIDTH + column]
    }

    /// The rectangles each entry wrote, merged from per-row runs, in VRAM order.
    pub fn regions(&self) -> Vec<Region> {
        // Runs of one owner per row, keyed by owner and column span, extended while the next row
        // repeats the same run.
        let mut open: BTreeMap<(u32, usize, usize), (usize, usize)> = BTreeMap::new();
        let mut done: Vec<(u32, usize, usize, usize, usize)> = Vec::new();
        for y in 0..VRAM_HEIGHT {
            let row = &self.owners[y * VRAM_WIDTH..(y + 1) * VRAM_WIDTH];
            let mut runs = Vec::new();
            let mut x = 0;
            while x < VRAM_WIDTH {
                let start = x;
                while x < VRAM_WIDTH && row[x] == row[start] {
                    x += 1;
                }
                if row[start] != NOBODY {
                    runs.push((row[start], start, x));
                }
            }
            let mut next = BTreeMap::new();
            for run in runs {
                let (top, _) = open.remove(&run).unwrap_or((y, y));
                next.insert(run, (top, y + 1));
            }
            for ((owner, left, right), (top, bottom)) in std::mem::replace(&mut open, next) {
                done.push((owner, left, right, top, bottom));
            }
        }
        done.extend(open.into_iter().map(|((owner, left, right), (top, bottom))| (owner, left, right, top, bottom)));
        done.sort_by_key(|&(_, left, _, top, _)| (top, left));
        done.into_iter()
            .filter_map(|(owner, left, right, top, bottom)| {
                Some(Region {
                    x: left as u16,
                    y: top as u16,
                    width: (right - left) as u16,
                    height: (bottom - top) as u16,
                    writer: self.writer_of(owner)?.clone(),
                    kind: self.kinds[owner as usize - 1],
                })
            })
            .collect()
    }

    /// The archives a [`Step::Load`] could not get.
    pub fn missing(&self) -> &[String] {
        &self.missing
    }

    /// Upload rectangles `EMI_UploadSectorImage` skipped because they leave the frame buffer.
    pub fn skipped(&self) -> usize {
        self.skipped
    }

    fn writer_of(&self, owner: u32) -> Option<&Writer> {
        owner.checked_sub(1).and_then(|index| self.writers.get(index as usize))
    }

    fn owner(&mut self, writer: Writer, kind: Kind) -> u32 {
        if let Some(index) = self.writers.iter().position(|seen| *seen == writer) {
            return index as u32 + 1;
        }
        self.writers.push(writer);
        self.kinds.push(kind);
        self.writers.len() as u32
    }

    fn upload(&mut self, writer: Writer, load_arg: u32, payload: &[u8]) {
        let Ok(rectangles) = vram::decode(payload, Descriptor::from_load_argument(load_arg)) else {
            return;
        };
        let owner = self.owner(writer, Kind::Upload);
        let side = vram::RECTANGLE_SIDE;
        let owners = vec![owner; side * side];
        for rectangle in rectangles {
            if rectangle.x >= UPLOAD_X_LIMIT || rectangle.y >= UPLOAD_Y_LIMIT {
                self.skipped += 1;
                continue;
            }
            self.write_rect(rectangle.x, rectangle.y, side, side, &rectangle.words, &owners);
        }
    }

    /// Writes a row-major rectangle of words and their owners, clipped to VRAM.
    fn write_rect(&mut self, x: usize, y: usize, width: usize, height: usize, words: &[u16], owners: &[u32]) {
        if x >= VRAM_WIDTH || width == 0 {
            return;
        }
        let clipped = width.min(VRAM_WIDTH - x);
        for row in 0..height.min(VRAM_HEIGHT.saturating_sub(y)) {
            let from = row * width;
            if from + clipped > words.len() || from + clipped > owners.len() {
                break;
            }
            let to = (y + row) * VRAM_WIDTH + x;
            self.words[to..to + clipped].copy_from_slice(&words[from..from + clipped]);
            self.owners[to..to + clipped].copy_from_slice(&owners[from..from + clipped]);
        }
    }

    fn write_ram(&mut self, writer: Writer, ram: u32, bytes: &[u8]) {
        if !overlaps_arena(ram, bytes.len()) {
            return;
        }
        let owner = self.owner(writer, Kind::Palette);
        let (pairs, _) = bytes.as_chunks::<2>();
        for (index, pair) in pairs.iter().enumerate() {
            let address = ram as u64 + 2 * index as u64;
            if address < u64::from(ARENA_BASE) {
                continue;
            }
            let word = ((address - u64::from(ARENA_BASE)) / 2) as usize;
            if word >= ARENA_WORDS {
                break;
            }
            self.arena[word] = u16::from_le_bytes(*pair);
            self.arena_owners[word] = owner;
        }
    }
}

/// Whether `len` bytes at RAM `ram` touch the palette arena.
fn overlaps_arena(ram: u32, len: usize) -> bool {
    let start = u64::from(ram);
    let end = start + len as u64;
    start < u64::from(ARENA_BASE + ARENA_BYTES) && end > u64::from(ARENA_BASE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(file_type: u16, ram_ptr: u32, size: usize) -> Entry {
        Entry { size: size as u32, offset: 0, ram_ptr, first4: 0, file_type }
    }

    /// One 32×32 upload at rectangle column `column`, row `row`, every word `value`.
    fn upload(column: u32, row: u32, value: u16) -> (Entry, Vec<u8>) {
        let payload: Vec<u8> = (0..1024).flat_map(|_| value.to_le_bytes()).collect();
        (entry(3, (column << 24) | (row << 16) | (1 << 8), payload.len()), payload)
    }

    fn bank(ram: u32, words: usize, value: u16) -> (Entry, Vec<u8>) {
        let payload: Vec<u8> = (0..words).flat_map(|_| value.to_le_bytes()).collect();
        (entry(0, ram, payload.len()), payload)
    }

    fn load(vram: &mut Vram, archive: &str, items: &[(Entry, Vec<u8>)]) {
        let entries: Vec<Entry> = items.iter().map(|(entry, _)| entry.clone()).collect();
        let payloads: Vec<&[u8]> = items.iter().map(|(_, payload)| payload.as_slice()).collect();
        vram.load(archive, &entries, &payloads);
    }

    #[test]
    fn a_fetch_reads_a_texel_through_its_clut() {
        let mut vram = Vram::new();
        // Word 0x4321 holds 4-bit texels 1, 2, 3, 4 and 8-bit texels 0x21, 0x43.
        vram.load_image(320, 256, 1, 1, &[0x4321], Writer::Named("page".into()));
        let colors: Vec<u16> = (0..256).map(|index| 0x1000 + index).collect();
        vram.load_image(0, 483, 256, 1, &colors, Writer::Named("clut".into()));
        let clut = Clut::new(0, 483).unwrap();
        assert_eq!(vram.fetch(320, 256, clut, Depth::Four, 0, 0), 0x1001);
        assert_eq!(vram.fetch(320, 256, clut, Depth::Four, 3, 0), 0x1004);
        assert_eq!(vram.fetch(320, 256, clut, Depth::Eight, 1, 0), 0x1043);
        assert_eq!(vram.fetch(320, 256, clut, Depth::Eight, 2, 0), 0x1000, "the next word is unwritten");
        vram.load_image(0, 0, 1, 1, &[0x0021], Writer::Named("origin".into()));
        assert_eq!(vram.fetch(1023, 511, clut, Depth::Eight, 2, 1), 0x1021, "u and v wrap to the origin");
    }

    #[test]
    fn an_upload_lands_at_its_rectangle_and_records_its_entry() {
        let mut vram = Vram::new();
        load(&mut vram, "A.EMI", &[bank(0x8000_0000, 4, 1), upload(2, 3, 0x1234)]);
        assert_eq!(vram.word(64, 96), 0x1234);
        assert_eq!(vram.word(95, 127), 0x1234);
        assert_eq!(vram.word(96, 96), 0);
        assert_eq!(vram.writer(64, 96), Some(&Writer::entry("A.EMI", 1)));
        assert_eq!(vram.writer(0, 0), None);
    }

    #[test]
    fn a_later_upload_overwrites_an_earlier_one() {
        let mut vram = Vram::new();
        load(&mut vram, "A.EMI", &[upload(0, 0, 1)]);
        load(&mut vram, "B.EMI", &[upload(0, 0, 2)]);
        assert_eq!(vram.word(0, 0), 2);
        assert_eq!(vram.writer(0, 0).and_then(Writer::as_entry), Some(("B.EMI", 0)));
    }

    #[test]
    fn the_loader_skips_a_rectangle_that_leaves_the_frame_buffer() {
        let mut vram = Vram::new();
        // Column 32 is x 1024 and row 16 is y 512: both outside.
        load(&mut vram, "A.EMI", &[upload(32, 0, 1), upload(0, 16, 1), upload(31, 15, 7)]);
        assert_eq!(vram.skipped(), 2);
        assert_eq!(vram.word(1023, 511), 7);
    }

    #[test]
    fn a_palette_reaches_vram_only_after_a_copy_and_an_upload() {
        let mut vram = Vram::new();
        // Arena row 3 (VRAM row 483).
        load(&mut vram, "A.EMI", &[bank(ARENA_BASE + 3 * 0x200, 16, 0x7C00)]);
        vram.upload_palette();
        assert_eq!(vram.word(0, 483), 0, "nothing was pending");
        vram.copy_palette(2);
        vram.upload_palette();
        assert_eq!(vram.word(0, 483), 0, "row 2 does not carry row 3");
        vram.copy_palette(3);
        vram.upload_palette();
        assert_eq!(vram.word(0, 483), 0x7C00);
        assert_eq!(vram.word(15, 483), 0x7C00);
        assert_eq!(vram.word(16, 483), 0);
        let clut = Clut::new(0, 483).expect("a CLUT");
        assert_eq!(vram.clut_writer(clut, Depth::Four), Some(&Writer::entry("A.EMI", 0)));
        assert_eq!(vram.clut_writer(clut, Depth::Eight), None, "the 8-bit row is not all one bank");
    }

    #[test]
    fn a_reset_clears_rows_28_to_31_before_it_copies() {
        let mut vram = Vram::new();
        load(&mut vram, "A.EMI", &[bank(ARENA_BASE + 28 * 0x200, 256, 5), bank(ARENA_BASE, 256, 6)]);
        vram.reset_palette();
        vram.upload_palette();
        assert_eq!(vram.word(0, 480), 6);
        assert_eq!(vram.word(0, 508), 0);
        assert_eq!(vram.writer(0, 508), None);
    }

    #[test]
    fn copied_words_carry_only_their_span() {
        let mut vram = Vram::new();
        load(&mut vram, "A.EMI", &[bank(ARENA_BASE + 15 * 0x200, 256, 9)]);
        vram.copy_words(15, 240, 16);
        vram.upload_palette();
        assert_eq!(vram.word(239, 495), 0);
        assert_eq!(vram.word(240, 495), 9);
        assert_eq!(vram.word(255, 495), 9);
    }

    #[test]
    fn regions_merge_rows_of_one_writer() {
        let mut vram = Vram::new();
        load(&mut vram, "A.EMI", &[upload(0, 0, 1), upload(1, 0, 1)]);
        load(&mut vram, "B.EMI", &[bank(ARENA_BASE, 16, 3)]);
        vram.reset_palette();
        vram.upload_palette();
        let regions = vram.regions();
        let a: Vec<_> = regions
            .iter()
            .filter(|region| region.writer.as_entry().is_some_and(|(archive, _)| archive == "A.EMI"))
            .collect();
        assert_eq!(a.len(), 2);
        assert!(a.iter().all(|region| (region.width, region.height) == (32, 32)));
        let b = regions
            .iter()
            .find(|region| region.writer.as_entry().is_some_and(|(archive, _)| archive == "B.EMI"))
            .expect("the palette");
        assert_eq!((b.x, b.y, b.width, b.height, b.kind), (0, 480, 16, 1, Kind::Palette));
    }

    #[test]
    fn texels_read_a_page_at_a_depth() {
        let mut vram = Vram::new();
        load(&mut vram, "A.EMI", &[upload(0, 0, 0x4321)]);
        assert_eq!(vram.texels(0, 0, 1, 1, Depth::Four), [1, 2, 3, 4]);
        assert_eq!(vram.texels(0, 0, 1, 1, Depth::Eight), [0x21, 0x43]);
    }

    #[test]
    fn load_image_clips_at_the_right_edge_and_names_its_writer() {
        let mut vram = Vram::new();
        vram.load_image(1022, 0, 4, 2, &[1, 2, 3, 4, 5, 6, 7, 8], Writer::Named("test".into()));
        assert_eq!((vram.word(1022, 0), vram.word(1023, 0), vram.word(1023, 1)), (1, 2, 6));
        assert_eq!(vram.writer(1023, 1), Some(&Writer::Named("test".into())));
    }

    #[test]
    fn replay_records_an_archive_it_cannot_get() {
        let vram = Vram::replay(&[Step::Clear, Step::Load("BIN/ETC/NONE.EMI".into())], &mut |_| None);
        assert_eq!(vram.missing(), ["BIN/ETC/NONE.EMI"]);
    }
}
