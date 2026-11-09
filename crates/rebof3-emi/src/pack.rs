//! Writing containers: byte-exact repacking of an extracted folder, a manifest
//! or an explicit file list (`wiki: formats/emi#padding`).

use std::fs::{self, File};
use std::io::{BufWriter, Cursor, Read, Seek, Write};
use std::path::{Path, PathBuf};

use crate::emi::{Archive, HEADER_SIZE, MAGIC, SECTOR_SIZE, TOC_ENTRY_SIZE, align_sector, ensure_parent};
use crate::error::{Error, Result};
use crate::manifest::manifest_entries;
use psx_formats::{sep, vab};

/// TOC padding value and the byte that fills the space between the TOC and the
/// first payload in the US `Breath of Fire III (USA)` archives.
const HEADER_PAD: u16 = 0x2e2e;
/// Byte that fills the space between a payload and the next sector boundary in
/// the US `Breath of Fire III (USA)` archives.
const PAYLOAD_PAD: u8 = 0x5f;
/// Most entries a pack accepts.
const MAX_PACK_ENTRIES: usize = 255;
/// Entry type given to a file whose extension names no type.
const UNKNOWN_TYPE: u16 = 255;

/// One file to pack and the table fields it is stored under.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackEntry {
    pub path: PathBuf,
    pub file_type: u16,
    pub ram_ptr: u32,
}

/// One payload held in memory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Payload<'a> {
    pub bytes: &'a [u8],
    pub file_type: u16,
    pub ram_ptr: u32,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PackFolderOptions {
    pub default_type: Option<u16>,
    pub include_patterns: Vec<String>,
    pub exclude_patterns: Vec<String>,
}

/// A table entry plus the stream its payload is copied from.
struct Slot<R> {
    file_type: u16,
    ram_ptr: u32,
    size: u32,
    first4: u32,
    source: R,
}

/// Writes `slots` as a container; `out` receives exactly the retail layout.
fn write_archive<R: Read>(out: &mut impl Write, archive_version: u32, slots: Vec<Slot<R>>) -> Result<()> {
    let toc_end = table_end(slots.len())?;
    out.write_all(&(slots.len() as u32).to_le_bytes())?;
    out.write_all(&archive_version.to_le_bytes())?;
    out.write_all(MAGIC)?;
    for slot in &slots {
        out.write_all(&slot.size.to_le_bytes())?;
        out.write_all(&slot.ram_ptr.to_le_bytes())?;
        out.write_all(&slot.first4.to_le_bytes())?;
        out.write_all(&slot.file_type.to_le_bytes())?;
        out.write_all(&HEADER_PAD.to_le_bytes())?;
    }
    let mut position = toc_end;
    position = pad_to_sector(out, position, HEADER_PAD as u8)?;
    for mut slot in slots {
        let copied = std::io::copy(&mut slot.source, out)?;
        if copied != u64::from(slot.size) {
            return Err(Error::InvalidArchive("entry changed while packing"));
        }
        position = pad_to_sector(out, position + copied, PAYLOAD_PAD)?;
    }
    debug_assert_eq!(position % SECTOR_SIZE, 0);
    out.flush()?;
    Ok(())
}

/// Where the table ends for `count` entries, or why `count` cannot be packed.
fn table_end(count: usize) -> Result<u64> {
    if count == 0 || count > MAX_PACK_ENTRIES {
        return Err(Error::InvalidArchive("pack requires 1 through 255 entries"));
    }
    let toc_end = HEADER_SIZE + count as u64 * TOC_ENTRY_SIZE;
    if toc_end > SECTOR_SIZE {
        return Err(Error::InvalidArchive("entry table does not fit before data"));
    }
    Ok(toc_end)
}

/// Fills from `position` to the next sector boundary and returns that boundary.
fn pad_to_sector(out: &mut impl Write, position: u64, fill: u8) -> Result<u64> {
    let aligned = align_sector(position)?;
    let padding = usize::try_from(aligned - position).expect("padding is below one sector");
    out.write_all(&vec![fill; padding])?;
    Ok(aligned)
}

/// The first four payload bytes as a little-endian word, zero-extended.
fn first_word(bytes: &[u8]) -> u32 {
    let mut word = [0; 4];
    let count = bytes.len().min(word.len());
    word[..count].copy_from_slice(&bytes[..count]);
    u32::from_le_bytes(word)
}

/// Packs files into the container `output`.
pub fn pack(output: impl AsRef<Path>, inputs: &[PackEntry], archive_version: u32) -> Result<()> {
    table_end(inputs.len())?;
    let mut slots = Vec::with_capacity(inputs.len());
    for input in inputs {
        let size = u32::try_from(fs::metadata(&input.path)?.len()).map_err(|_| Error::InvalidArchive("entry exceeds 4 GiB"))?;
        let mut file = File::open(&input.path)?;
        let mut head = [0; 4];
        let count = file.read(&mut head)?;
        file.rewind()?;
        slots.push(Slot { file_type: input.file_type, ram_ptr: input.ram_ptr, size, first4: first_word(&head[..count]), source: file });
    }
    ensure_parent(output.as_ref())?;
    let mut out = BufWriter::new(File::create(output.as_ref())?);
    write_archive(&mut out, archive_version, slots)
}

/// Rebuilds an EMI container from its own entries, with the entries named in `replace` swapped for new payloads.
/// Every entry keeps its type and load address, and the container keeps its version. With no replacement the
/// result is the container as its entries pack.
pub fn rebuild(container: &[u8], replace: &[(usize, &[u8])]) -> Result<Vec<u8>> {
    rebuild_placed(container, replace, &[])
}

/// [`rebuild`] with a new load address for each entry named in `placed`: the one field of the table a
/// texture or palette move changes. An entry not named keeps its load address.
pub fn rebuild_placed(container: &[u8], replace: &[(usize, &[u8])], placed: &[(usize, u32)]) -> Result<Vec<u8>> {
    let archive = Archive::parse(container)?;
    let count = archive.entries().len();
    let named = replace.iter().map(|(index, _)| *index).chain(placed.iter().map(|(index, _)| *index));
    if let Some(index) = named.into_iter().find(|index| *index >= count) {
        return Err(Error::EntryOutOfRange { index, count });
    }
    let mut payloads = Vec::with_capacity(count);
    for (index, entry) in archive.entries().iter().enumerate() {
        let bytes = match replace.iter().find(|(at, _)| *at == index) {
            Some((_, bytes)) => *bytes,
            None => archive.payload(container, index)?,
        };
        let ram_ptr = placed.iter().find(|(at, _)| *at == index).map_or(entry.ram_ptr, |(_, ram)| *ram);
        payloads.push(Payload { bytes, file_type: entry.file_type, ram_ptr });
    }
    pack_payloads(archive.version(), &payloads)
}

/// Packs in-memory payloads into container bytes.
pub fn pack_payloads(archive_version: u32, payloads: &[Payload<'_>]) -> Result<Vec<u8>> {
    let mut slots = Vec::with_capacity(payloads.len());
    for payload in payloads {
        slots.push(Slot {
            file_type: payload.file_type,
            ram_ptr: payload.ram_ptr,
            size: u32::try_from(payload.bytes.len()).map_err(|_| Error::InvalidArchive("entry exceeds 4 GiB"))?,
            first4: first_word(payload.bytes),
            source: Cursor::new(payload.bytes),
        });
    }
    let mut out = Vec::new();
    write_archive(&mut out, archive_version, slots)?;
    Ok(out)
}

/// Packs the entries `manifest_path` names, resolved against its own folder.
pub fn pack_manifest(output: impl AsRef<Path>, manifest_path: impl AsRef<Path>) -> Result<()> {
    let manifest_path = manifest_path.as_ref();
    let root = manifest_path.parent().unwrap_or(Path::new("."));
    pack_manifest_from(output, root, manifest_path)
}

/// Packs the entries `manifest_path` names, resolved against `folder`.
pub fn pack_manifest_from(output: impl AsRef<Path>, folder: impl AsRef<Path>, manifest_path: impl AsRef<Path>) -> Result<()> {
    let (inputs, archive_version) = manifest_entries(folder, manifest_path)?;
    pack(output, &inputs, archive_version)
}

/// Packs the regular files of `folder` in name order, typed by extension.
pub fn pack_folder(output: impl AsRef<Path>, folder: impl AsRef<Path>, options: &PackFolderOptions) -> Result<()> {
    let mut inputs = Vec::new();
    for entry in fs::read_dir(folder.as_ref())? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !options.include_patterns.is_empty() && !options.include_patterns.iter().any(|pattern| glob_match(pattern, &name)) {
            continue;
        }
        if options.exclude_patterns.iter().any(|pattern| glob_match(pattern, &name)) {
            continue;
        }
        let guessed = guess_type(entry.path());
        inputs.push(PackEntry { path: entry.path(), file_type: guessed.or(options.default_type).unwrap_or(UNKNOWN_TYPE), ram_ptr: 0 });
    }
    inputs.sort_by(|left, right| left.path.cmp(&right.path));
    pack(output, &inputs, 0)
}

/// The entry type a file extension implies, if it names one.
pub fn guess_type(path: impl AsRef<Path>) -> Option<u16> {
    let extension = path.as_ref().extension();
    let Some(extension) = extension else {
        return Some(0);
    };
    match extension.to_string_lossy().to_ascii_lowercase().as_str() {
        "img" | "tim" => Some(crate::emi::TYPE_VRAM),
        "vh" => Some(vab::HEADER_TYPE),
        "vb" => Some(vab::BODY_TYPE),
        "seq" | "mid" => Some(sep::TYPE),
        "lz77" => Some(crate::emi::TYPE_LZ77),
        "emi8" => Some(crate::emi::TYPE_CUE),
        "bin" | "dat" | "clut" | "map" | "emi0" => Some(0),
        _ => None,
    }
}

/// Shell-style match of `text` against `pattern` (`*` and `?` only).
pub fn glob_match(pattern: &str, text: &str) -> bool {
    fn matches(pattern: &[u8], text: &[u8]) -> bool {
        match pattern.split_first() {
            None => text.is_empty(),
            Some((&b'*', rest)) => matches(rest, text) || (!text.is_empty() && matches(pattern, &text[1..])),
            Some((&b'?', rest)) => !text.is_empty() && matches(rest, &text[1..]),
            Some((&byte, rest)) => text.first() == Some(&byte) && matches(rest, &text[1..]),
        }
    }
    matches(pattern.as_bytes(), text.as_bytes())
}
