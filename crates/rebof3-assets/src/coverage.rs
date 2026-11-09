//! Round-trip coverage: how much of the disc's EMI content exports and imports to its own bytes.
//!
//! For each kind, the line counts the entries of that kind on the disc and the bytes they hold, and the part of them
//! that round trips through the kind's own format. A kind with no round trip here reports zero, and [`has_round_trip`]
//! says which kinds those are. The text line counts the entries that hold game strings, outside message tables and
//! inside them. The `ETC/GAME.EMI` tables are counted one table at a time.

use std::collections::BTreeMap;
use std::path::Path;

use psx_cd::Image;
use rebof3_emi::Archive;
use rebof3_formats::game_tables::TABLES;
use rebof3_formats::{Format, classify};

use crate::error::Result;
use crate::kind::Kind;
use rebof3_formats::message::table;
use rebof3_formats::text::{discover, strings};

/// The disc path of the fixed game tables, whose eight tables are the data-table entries.
const GAME_TABLES_SOURCE: &str = "BIN/ETC/GAME.EMI";
/// The slot of the fixed game tables in that archive.
const GAME_TABLES_SLOT: usize = 0;
/// The disc path of the playable characters' base-stat records, and their slot in that archive.
const CHARACTERS_SOURCE: &str = "BIN/ETC/START.EMI";
const CHARACTERS_SLOT: usize = 8;

/// One line: the entries and bytes on the disc, and the part that round trips.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Line {
    pub entries: usize,
    pub bytes: u64,
    pub both_entries: usize,
    pub both_bytes: u64,
}

impl Line {
    fn count(&mut self, bytes: u64, both: bool) {
        self.count_part(bytes, if both { bytes } else { 0 }, both);
    }

    /// Counts one entry of `bytes` bytes, of which `both_bytes` round trip. `both` says whether the entry counts.
    fn count_part(&mut self, bytes: u64, both_bytes: u64, both: bool) {
        self.entries += 1;
        self.bytes += bytes;
        self.both_bytes += both_bytes;
        if both {
            self.both_entries += 1;
        }
    }
}

/// The lines of a disc: one per kind that has entries, the text line, and the palette line of the character sprites.
#[derive(Debug, Default)]
pub struct Report {
    pub kinds: BTreeMap<Kind, Line>,
    pub text: Line,
    /// The palette tables of the character sprites, against their decoded bytes. The node data is not decoded.
    pub palettes: Line,
}

/// Whether the kind has a round trip in this module. The other kinds report their entries with zero round-tripped.
pub fn has_round_trip(kind: Kind) -> bool {
    matches!(kind, Kind::AreaMessage | Kind::AreaEncounters | Kind::DataTable | Kind::Text)
}

/// Measures the round trips of every EMI entry on the disc at `image`.
pub fn run(image: &Path) -> Result<Report> {
    let mut disc = Image::open(image)?;
    let mut report = Report::default();
    for file in disc.entries()? {
        let is_emi = file.path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("EMI"));
        if file.is_directory || file.is_xa || !is_emi {
            continue;
        }
        let source = file.path.display().to_string();
        let container = disc.read_file(&file)?;
        let Ok(archive) = Archive::parse(&container) else { continue };
        for (index, entry) in archive.entries().iter().enumerate() {
            let payload = archive.payload(&container, index)?;
            let format = classify(entry, payload);
            let both = format_round_trips(format, payload);
            report.kinds.entry(Kind::of_format(format)).or_default().count(payload.len() as u64, both);
            if strings::holds_text(entry)
                && let Some(text) = text_round_trips(entry, payload)
            {
                report.text.count(payload.len() as u64, text);
            }
            if matches!(format, Format::CharacterSpritesLz77 | Format::CharacterSprites) {
                palettes(&mut report, format, payload);
            }
            if source == GAME_TABLES_SOURCE && index == GAME_TABLES_SLOT {
                game_tables(&mut report, payload)?;
            }
            if source == CHARACTERS_SOURCE && index == CHARACTERS_SLOT {
                characters(&mut report, payload);
            }
        }
    }
    Ok(report)
}

/// Whether an entry of this format exports and imports to its own bytes. Formats without a round trip are `false`.
fn format_round_trips(format: Format, payload: &[u8]) -> bool {
    match format {
        Format::AreaMessageTable => table_round_trips(payload),
        Format::AreaEncounterTable => rebof3_formats::encounter::open::export(payload).and_then(|json| rebof3_formats::encounter::open::import(&json, payload)).is_ok_and(|rebuilt| rebuilt == payload),
        _ => false,
    }
}

/// The palette tables of character sprites: the decoded bytes, and its palette tables when they export and import to
/// the decoded bytes. A container that is not in the palette layout is not counted.
fn palettes(report: &mut Report, format: Format, payload: &[u8]) {
    match format {
        Format::CharacterSpritesLz77 => {
            let Ok(decoded) = rebof3_emi::lz77::decode(payload) else { return };
            palette_tables(report, &decoded);
        }
        Format::CharacterSprites => palette_tables(report, payload),
        _ => (),
    }
}

fn palette_tables(report: &mut Report, decoded: &[u8]) {
    let Ok(sprites) = rebof3_formats::sprite_palettes::parse(decoded) else { return };
    let both = rebof3_formats::sprite_palettes::export(decoded).and_then(|json| rebof3_formats::sprite_palettes::import(&json, decoded)).is_ok_and(|rebuilt| rebuilt == decoded);
    let typed = if both { sprites.table_bytes() as u64 } else { 0 };
    report.palettes.count_part(decoded.len() as u64, typed, both);
}

fn table_round_trips(payload: &[u8]) -> bool {
    table::export(payload).and_then(|json| table::import(&json)).is_ok_and(|rebuilt| rebuilt == payload)
}

/// The text of an entry: `None` when it holds none, otherwise whether that text round trips.
fn text_round_trips(entry: &rebof3_emi::Entry, payload: &[u8]) -> Option<bool> {
    if let Some(records) = discover::table_records(payload) {
        return (!records.is_empty()).then(|| table_round_trips(payload));
    }
    let base = strings::load_address(entry);
    if discover::strings(payload, base).is_empty() {
        return None;
    }
    Some(strings::export(payload, base).and_then(|json| strings::import(&json, payload)).is_ok_and(|rebuilt| rebuilt == payload))
}

/// Each record of the playable characters: its stored bytes, and whether the table's JSON reads back to them.
fn characters(report: &mut Report, payload: &[u8]) {
    let both = rebof3_formats::characters::export(payload).and_then(|json| rebof3_formats::characters::import(&json, payload)).is_ok_and(|rebuilt| rebuilt == payload);
    for _ in 0..rebof3_formats::characters::CHARACTERS {
        report.kinds.entry(Kind::DataTable).or_default().count(rebof3_formats::characters::RECORD_BYTES as u64, both);
    }
}

/// Each fixed table of the game tables entry: its stored bytes, and whether its JSON reads back to them.
fn game_tables(report: &mut Report, payload: &[u8]) -> Result<()> {
    let files = rebof3_formats::game_tables::open::files(payload)?;
    for table in TABLES {
        let start = table.archive_offset - rebof3_formats::game_tables::PAYLOAD_START;
        let stored = &payload[start..start + table.count * table.record];
        let json = files.iter().find(|(name, _)| *name == table.name).map(|(_, json)| json.as_slice());
        let both = json.is_some_and(|json| rebof3_formats::game_tables::open::import_in_payload(table.name, json, payload).is_ok_and(|rebuilt| rebuilt == stored));
        report.kinds.entry(Kind::DataTable).or_default().count(stored.len() as u64, both);
    }
    Ok(())
}
