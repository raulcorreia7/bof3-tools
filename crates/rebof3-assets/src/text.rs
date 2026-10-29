//! The `text` kind: game strings.
//!
//! [`codec`] reads and writes the game's byte encodings, and [`bank`] the message banks that hold the dialogue.
//! [`extract`] walks the extracted disc (`build/disc`, or `--text-disc DIR`) and writes, for every payload that
//! holds text, the stored bytes, one JSON row per string and a plain-text view. [`known`] places the strings of
//! the documented tables, and the scan finds the loose ones.

pub mod bank;
pub mod codec;
pub mod known;
pub mod source;
pub mod strings;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use psx_cd::Image;
use psx_cd::checksum::sha256_hex;
use rebof3_emi::Archive;
use rebof3_emi::Entry;
use rebof3_emi::classify::{Format, classify};
use rebof3_emi::text::discover;

use crate::error::{Error, Result};
use crate::kind::Kind;
use crate::report::{Report, Row};
use source::PayloadSource;

/// Directory of the text tree below the output directory.
pub const TEXT_DIR: &str = "text";
/// Default extracted disc, relative to the working directory.
pub const DEFAULT_DISC: &str = "build/disc";
/// The index of every string the run wrote.
pub const INDEX_FILE: &str = "index.jsonl";
/// The format name of the text index in `assets.jsonl`.
pub const INDEX_FORMAT: &str = "text index";

/// What a run writes.
#[derive(Clone, Debug)]
pub struct Options {
    /// The extracted disc the text is read from.
    pub disc: PathBuf,
    /// The output directory; the files go below its [`TEXT_DIR`].
    pub out: PathBuf,
    /// The disc image, for the archive's LBA. Without it the JSON omits the disc offset.
    pub image: Option<PathBuf>,
    /// Write each payload's stored bytes as `<slot>.bin`.
    pub raw: bool,
    /// Write the JSON, the plain text and the run's index.
    pub modern: bool,
    /// `false` plans the run: nothing is written.
    pub write: bool,
    /// List every payload the scan drops (the `--list` plan).
    pub list: bool,
}

impl Options {
    /// Both forms, from `disc` into `out`, with no disc image for the LBA.
    fn both(disc: &Path, out: &Path, write: bool) -> Self {
        Self {
            disc: disc.to_path_buf(),
            out: out.to_path_buf(),
            image: None,
            raw: true,
            modern: true,
            write,
            list: !write,
        }
    }
}

/// One text payload's files and the rows they hold.
struct TextFile {
    payload: Vec<u8>,
    json: Vec<u8>,
    plain: String,
    index: Vec<IndexRow>,
    strings: usize,
}

/// One string as the plain `<slot>.txt` lists it.
struct Line {
    offset: usize,
    id: String,
    text: String,
}

/// One string as the run's `text/index.jsonl` lists it.
#[derive(serde::Serialize)]
struct IndexRow {
    source: String,
    kind: String,
    id: String,
    ram: Option<String>,
    text: String,
}

/// Writes the strings of every payload of the extracted disc `disc` below `<out>/text`: the stored bytes, the
/// JSON and a plain-text view for each payload that holds text. `write` false plans the run.
pub fn extract(disc: &Path, out: &Path, write: bool) -> Result<Report> {
    extract_with(&Options::both(disc, out, write))
}

/// Writes what `options` selects.
pub fn extract_with(options: &Options) -> Result<Report> {
    if !options.disc.is_dir() {
        return Err(Error::Text(format!(
            "no extracted disc at {}; pass --text-disc DIR with the tree `rebof3 disc extract` wrote",
            options.disc.display()
        )));
    }
    let mut report = Report::default();
    let mut archives = Vec::new();
    archives_under(&options.disc, &mut archives)?;
    let lba = lba_map(options.image.as_deref());
    let (mut files, mut found, mut dropped, mut skipped) = (0usize, 0usize, 0usize, 0usize);
    let mut index = Vec::new();
    for path in archives {
        let source = source_of(&options.disc, &path);
        match archive_rows(&path, &source, options, lba.get(&source).copied(), &mut index, &mut report) {
            Ok(counts) => {
                files += counts.files;
                found += counts.strings;
                dropped += counts.dropped;
                skipped += counts.skipped;
            }
            Err(error) => report.failures.push(format!("{source}: {error}")),
        }
    }
    if options.modern && options.write {
        write_index(options, &mut index, &mut report)?;
    }
    report.notes.push(format!(
        "text: {files} payloads, {found} strings, {dropped} dropped from the scan, {skipped} graphics/audio entries"
    ));
    report.sort();
    Ok(report)
}

#[derive(Default)]
struct Counts {
    files: usize,
    strings: usize,
    dropped: usize,
    skipped: usize,
}

/// Every `.EMI` archive under `directory`, in path order.
fn archives_under(directory: &Path, found: &mut Vec<PathBuf>) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(directory)?.collect::<std::io::Result<_>>()?;
    entries.sort_by_key(fs::DirEntry::path);
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            archives_under(&path, found)?;
        } else if path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("EMI")) {
            found.push(path);
        }
    }
    Ok(())
}

/// The disc path of `path` below `disc`, with forward slashes.
fn source_of(disc: &Path, path: &Path) -> String {
    path.strip_prefix(disc).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

/// The archive name without its `.EMI` extension.
fn stem(source: &str) -> &str {
    match source.rsplit_once('.') {
        Some((stem, extension)) if extension.eq_ignore_ascii_case("EMI") => stem,
        _ => source,
    }
}

/// The archive file's first sector, from the disc image's file table.
fn lba_map(image: Option<&Path>) -> BTreeMap<String, u32> {
    let Some(image) = image else { return BTreeMap::new() };
    let Ok(mut image) = Image::open(image) else { return BTreeMap::new() };
    let Ok(entries) = image.entries() else { return BTreeMap::new() };
    entries
        .into_iter()
        .filter(|entry| !entry.is_directory)
        .map(|entry| (entry.path.to_string_lossy().replace('\\', "/"), entry.lba))
        .collect()
}

/// Writes one archive's text files.
fn archive_rows(
    path: &Path,
    source: &str,
    options: &Options,
    lba: Option<u32>,
    index: &mut Vec<IndexRow>,
    report: &mut Report,
) -> Result<Counts> {
    let container = fs::read(path)?;
    let archive = Archive::parse(&container)?;
    let mut counts = Counts::default();
    for (slot, entry) in archive.entries().iter().enumerate() {
        if !strings::holds_text(entry) {
            counts.skipped += 1;
            continue;
        }
        let payload = archive.payload(&container, slot)?;
        let format = classify(entry, payload);
        let source = PayloadSource {
            source: source.to_string(),
            entry: slot,
            entry_offset: entry.offset,
            lba,
            load: strings::load_address(entry),
        };
        let text = match text_file(&source, slot, entry, payload, format) {
            Ok(Some(text)) => text,
            Ok(None) => {
                counts.dropped += 1;
                if options.list {
                    report.notes.push(format!("text scan drop: {}#{slot} {}", source.source, format.name()));
                }
                continue;
            }
            Err(error) => {
                report.failures.push(format!("{}#{slot}: {error}", source.source));
                continue;
            }
        };
        let base = format!("{TEXT_DIR}/{}/{}", stem(&source.source), slot);
        if options.raw {
            let output = format!("{base}.bin");
            write_bytes(options, &output, &text.payload)?;
            report.rows.push(row(&source, entry, payload, output, &text.payload, options.write, "raw"));
        }
        if options.modern {
            let output = format!("{base}.json");
            write_bytes(options, &output, &text.json)?;
            report.rows.push(row(&source, entry, payload, output, &text.json, options.write, "json"));
            let output = format!("{base}.txt");
            write_bytes(options, &output, text.plain.as_bytes())?;
            report.rows.push(row(&source, entry, payload, output, text.plain.as_bytes(), options.write, "txt"));
            index.extend(text.index);
        }
        counts.files += 1;
        counts.strings += text.strings;
    }
    Ok(counts)
}

/// One payload's stored bytes, JSON rows and plain-text view, or `None` when it holds no text.
fn text_file(
    source: &PayloadSource,
    slot: usize,
    entry: &Entry,
    payload: &[u8],
    format: Format,
) -> Result<Option<TextFile>> {
    let known = known::strings(source, slot, format, payload);
    let exclude: Vec<(usize, usize)> = known.iter().map(known::Known::range).collect();
    if format == Format::AreaMessageBank {
        if discover::bank_records(payload).is_none_or(|records| records.is_empty()) {
            return Ok(None);
        }
        let (json, strings, rows) = bank::export_counted(payload, Some(source))?;
        if strings == 0 {
            return Ok(None);
        }
        let lines: Vec<Line> = rows
            .iter()
            .map(|row| Line { offset: row.origin.offset, id: row.id.clone(), text: row.text.clone() })
            .collect();
        let index = rows.iter().map(|row| index_row(source, &row.origin, &row.kind, &row.id, &row.text)).collect();
        return Ok(Some(TextFile { payload: payload.to_vec(), plain: plain_text(&lines), json, index, strings }));
    }

    let scan = strings::rows(payload, strings::load_address(entry), Some(source), &exclude)?;
    if known.is_empty() && !scan_holds_text(format, &scan) {
        return Ok(None);
    }
    let mut items = Vec::with_capacity(known.len() + scan.len());
    for placed in &known {
        let raw = &payload[placed.offset..placed.offset + placed.length];
        items.push(strings::Item::known(
            source.origin(placed.offset),
            raw,
            placed.stride,
            placed.text.clone(),
            placed.table,
            placed.index,
        )?);
    }
    items.extend(scan);
    items.sort_by_key(|item| item.origin.offset);
    let lines: Vec<Line> = items
        .iter()
        .map(|item| Line { offset: item.origin.offset, id: item.id.clone(), text: item.text.clone() })
        .collect();
    let index = items.iter().map(|item| index_row(source, &item.origin, &item.kind, &item.id, &item.text)).collect();
    Ok(Some(TextFile {
        payload: payload.to_vec(),
        json: strings::document(payload.len(), &items),
        plain: plain_text(&lines),
        index,
        strings: items.len(),
    }))
}

/// Whether a payload's scanned strings are text, rather than bytes that happen to decode as some.
///
/// A resource table is a string table by construction. A code image is kept when it holds a run of
/// at least four short labels, which is what a menu table looks like, or a loose record with two
/// words; a lone mixed-case run in a sprite or an area overlay is not text.
fn scan_holds_text(format: Format, scan: &[strings::Item]) -> bool {
    match format {
        Format::SharedResourceTable => !scan.is_empty(),
        Format::CodeImage => {
            let fields = scan.iter().filter(|item| item.kind == "string-table" && label_like(&item.text)).count();
            fields >= 4 || scan.iter().any(|item| item.kind == "loose" && words_like(&item.text))
        }
        _ => false,
    }
}

/// Whether a short table label has enough letters to be a word.
fn label_like(text: &str) -> bool {
    let plain = untagged(text);
    let letters = plain.chars().filter(|character| character.is_alphabetic()).count();
    letters >= 3 && controls(text) * 2 <= letters
}

/// Whether a loose record reads as at least two words.
fn words_like(text: &str) -> bool {
    let plain = untagged(text);
    let letters = plain.chars().filter(|character| character.is_alphabetic()).count();
    let words = plain
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| word.chars().any(|character| character.is_alphabetic()))
        .count();
    words >= 2 && letters >= 5 && controls(text) * 2 <= letters
}

/// The text with its `{TAG}` markers removed.
fn untagged(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut in_tag = false;
    for character in text.chars() {
        match character {
            '{' => in_tag = true,
            '}' if in_tag => in_tag = false,
            _ if !in_tag => plain.push(character),
            _ => {}
        }
    }
    plain
}

/// How many `{TAG}` markers the text holds.
fn controls(text: &str) -> usize {
    text.matches('{').count()
}

/// The lines of one payload's plain-text view: `offset<TAB>id<TAB>text`.
fn plain_text(lines: &[Line]) -> String {
    let mut out = String::new();
    for line in lines {
        use std::fmt::Write as _;
        let _ = writeln!(out, "{:#x}\t{}\t{}", line.offset, line.id, line.text);
    }
    out
}

/// One row of the run's index.
fn index_row(source: &PayloadSource, origin: &source::Origin, kind: &str, id: &str, text: &str) -> IndexRow {
    IndexRow {
        source: format!("{}#{}+{:#x}", source.source, source.entry, origin.offset),
        kind: kind.to_string(),
        id: id.to_string(),
        ram: origin.ram.map(|ram| format!("{ram:#010x}")),
        text: text.to_string(),
    }
}

/// Writes the run's `text/index.jsonl`.
fn write_index(options: &Options, index: &mut [IndexRow], report: &mut Report) -> Result<()> {
    index.sort_by(|left, right| (&left.source, &left.id).cmp(&(&right.source, &right.id)));
    let mut jsonl = String::new();
    for row in index.iter() {
        let line = serde_json::to_string(row).map_err(|error| Error::Text(error.to_string()))?;
        jsonl.push_str(&line);
        jsonl.push('\n');
    }
    let output = format!("{TEXT_DIR}/{INDEX_FILE}");
    write_bytes(options, &output, jsonl.as_bytes())?;
    report.rows.push(Row {
        source: output.clone(),
        slot: None,
        offset: 0,
        size: jsonl.len() as u64,
        file_type: None,
        load_arg: None,
        format: INDEX_FORMAT,
        kind: Kind::Text,
        state: "codec",
        role: "index",
        output: Some(output),
        bytes: jsonl.len() as u64,
        sha256: sha256_hex(jsonl.as_bytes()),
        evidence: String::new(),
        note: String::new(),
    });
    Ok(())
}

/// Writes `bytes` below the output directory, creating its directory, unless the run only plans.
fn write_bytes(options: &Options, output: &str, bytes: &[u8]) -> Result<()> {
    if !options.write {
        return Ok(());
    }
    let path = options.out.join(output);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, bytes)?;
    Ok(())
}

/// The `assets.jsonl` row of one written file.
fn row(
    source: &PayloadSource,
    entry: &Entry,
    payload: &[u8],
    output: String,
    written: &[u8],
    write: bool,
    role: &'static str,
) -> Row {
    let format = classify(entry, payload);
    Row {
        source: source.source.clone(),
        slot: Some(source.entry),
        offset: entry.offset,
        size: u64::from(entry.size),
        file_type: Some(entry.file_type),
        load_arg: Some(entry.ram_ptr),
        format: format.name(),
        kind: Kind::Text,
        // The text codec produced the JSON, whatever the payload's own format state is.
        state: "codec",
        role,
        output: Some(output),
        bytes: written.len() as u64,
        sha256: if write { sha256_hex(written) } else { String::new() },
        evidence: String::new(),
        note: String::new(),
    }
}
