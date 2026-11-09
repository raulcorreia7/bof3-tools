//! `assets.jsonl` and the by-kind summary.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::kind::{Handling, Kind};

/// Keys of each `assets.jsonl` object, in column order.
pub const COLUMNS: [&str; 15] = [
    "source", "slot", "offset", "size", "type", "load_arg", "format", "kind", "state", "role", "output", "bytes",
    "sha256", "evidence", "note",
];
/// Marks a column that does not apply to the row.
pub const NONE: &str = "-";
/// The `format` of a file rendered from a `bof3player` catalog item.
pub const RENDERED_ITEM: &str = "bof3player item";
/// The `format` of a file derived from a game table in `ETC/GAME.EMI` entry 0.
pub const DERIVED_TABLE: &str = "game table (derived)";
/// The `format` of the text run's index.
pub const TEXT_INDEX: &str = "text index";

/// Whether a row's file is an output of an item or entry, not an entry of its own.
pub fn is_output_only(format: &str) -> bool {
    format == RENDERED_ITEM || format == DERIVED_TABLE || format == TEXT_INDEX
}

/// One written file, or one payload that wrote nothing, and its source.
#[derive(Clone, Debug)]
pub struct Row {
    /// Disc path of the file the payload comes from.
    pub source: String,
    /// EMI slot, or `None` for a whole disc file.
    pub slot: Option<usize>,
    /// Byte offset of the payload in its source file.
    pub offset: u64,
    /// Payload size in bytes.
    pub size: u64,
    /// EMI entry type.
    pub file_type: Option<u16>,
    /// EMI load argument.
    pub load_arg: Option<u32>,
    /// The census format name (`rebof3_formats::Format::name`).
    pub format: &'static str,
    pub kind: Kind,
    /// The census state: codec, specified, unknown, code, reserve, or file.
    pub state: &'static str,
    /// What the output is: raw, png, json, decoded.
    pub role: &'static str,
    /// Path of the written file below the output directory.
    pub output: Option<String>,
    /// Size of the written file.
    pub bytes: u64,
    pub sha256: String,
    /// The loader evidence behind a derived file: the draw site that joins it, or empty.
    pub evidence: String,
    /// Why a derived file is missing, when it is.
    pub note: String,
}

impl Row {
    fn cells(&self) -> [String; 15] {
        let optional = |value: Option<String>| value.unwrap_or_else(|| NONE.to_string());
        [
            self.source.clone(),
            optional(self.slot.map(|slot| slot.to_string())),
            self.offset.to_string(),
            self.size.to_string(),
            optional(self.file_type.map(|file_type| file_type.to_string())),
            optional(self.load_arg.map(|value| format!("{value:#010x}"))),
            self.format.to_string(),
            self.kind.name().to_string(),
            self.state.to_string(),
            self.role.to_string(),
            optional(self.output.clone()),
            self.bytes.to_string(),
            if self.sha256.is_empty() { NONE.to_string() } else { self.sha256.clone() },
            if self.evidence.is_empty() { NONE.to_string() } else { self.evidence.clone() },
            if self.note.is_empty() { NONE.to_string() } else { self.note.clone() },
        ]
    }
}

/// Everything a run produced.
#[derive(Debug, Default)]
pub struct Report {
    pub rows: Vec<Row>,
    /// Payloads whose derived output could not be written, one line each.
    pub failures: Vec<String>,
    /// Lines on kinds another tool owns or the run could not produce.
    pub notes: Vec<String>,
}

impl Report {
    /// Orders the rows: by source, slot, then output name.
    pub fn sort(&mut self) {
        self.rows.sort_by(|left, right| {
            (&left.source, left.slot, &left.output).cmp(&(&right.source, right.slot, &right.output))
        });
    }

    /// The `assets.jsonl` text: one JSON object per row, keyed by [`COLUMNS`].
    pub fn jsonl(&self) -> String {
        let mut out = String::new();
        for row in &self.rows {
            let record: serde_json::Map<String, serde_json::Value> = COLUMNS
                .iter()
                .zip(row.cells())
                .map(|(key, cell)| ((*key).to_string(), serde_json::Value::String(cell)))
                .collect();
            out.push_str(&serde_json::Value::Object(record).to_string());
            out.push('\n');
        }
        out
    }

    /// Payloads per kind.
    fn payloads(&self) -> BTreeMap<Kind, Tally> {
        let mut tallies: BTreeMap<Kind, Tally> = BTreeMap::new();
        let mut seen = std::collections::HashSet::new();
        for row in &self.rows {
            let tally = tallies.entry(row.kind).or_default();
            tally.files += usize::from(row.output.is_some());
            tally.written += row.bytes;
            tally.state = row.state;
            if !is_output_only(row.format) && seen.insert((&row.source, row.slot)) {
                tally.entries += 1;
                tally.bytes += row.size;
            }
        }
        tallies
    }

    /// The summary by kind, the coverage and classified lines, and every unknown entry.
    pub fn summary(&self) -> String {
        let tallies = self.payloads();
        let mut out = String::new();
        let _ = writeln!(
            out,
            "{:<17} {:>7} {:>13} {:>7} {:>13}  {:<9} handling",
            "kind", "entries", "bytes", "files", "written", "state"
        );
        let mut total = Tally::default();
        for kind in Kind::ALL {
            let Some(tally) = tallies.get(kind) else {
                continue;
            };
            let _ = writeln!(
                out,
                "{:<17} {:>7} {:>13} {:>7} {:>13}  {:<9} {}",
                kind.name(),
                tally.entries,
                tally.bytes,
                tally.files,
                tally.written,
                tally.state,
                kind.handling().label(),
            );
            total.entries += tally.entries;
            total.bytes += tally.bytes;
            total.files += tally.files;
            total.written += tally.written;
        }
        let _ = writeln!(
            out,
            "{:<17} {:>7} {:>13} {:>7} {:>13}",
            "TOTAL", total.entries, total.bytes, total.files, total.written
        );
        let coverage = self.coverage(&tallies);
        let _ = writeln!(out, "{coverage}");
        let unknown = self.unknown_entries();
        let _ = writeln!(
            out,
            "classified: {} of {} entries, {} of {} bytes",
            total.entries.saturating_sub(unknown.total),
            total.entries,
            total.bytes.saturating_sub(unknown.bytes),
            total.bytes
        );
        let _ = writeln!(out, "unknown entries: {} ({} bytes), all listed:", unknown.total, unknown.bytes);
        for row in &unknown.rows {
            let _ = writeln!(
                out,
                "  {:>10} bytes  {}#{}  type {} load {}  {}",
                row.size,
                row.source,
                row.slot.map_or(NONE.to_string(), |slot| slot.to_string()),
                row.file_type.map_or(NONE.to_string(), |value| value.to_string()),
                row.load_arg.map_or(NONE.to_string(), |value| format!("{value:#010x}")),
                row.format,
            );
        }
        for failure in &self.failures {
            let _ = writeln!(out, "failure: {failure}");
        }
        for note in &self.notes {
            let _ = writeln!(out, "note: {note}");
        }
        out
    }

    /// One line: how much of the disc each handling covers.
    fn coverage(&self, tallies: &BTreeMap<Kind, Tally>) -> String {
        let mut by_handling: BTreeMap<&str, (usize, u64)> = BTreeMap::new();
        for (kind, tally) in tallies {
            let slot = by_handling.entry(kind.handling().label()).or_default();
            slot.0 += tally.entries;
            slot.1 += tally.bytes;
        }
        let parts: Vec<String> = [Handling::Decoded, Handling::Raw, Handling::Nothing]
            .iter()
            .filter_map(|handling| {
                by_handling
                    .get(handling.label())
                    .map(|(entries, bytes)| format!("{} {entries} entries/{bytes} bytes", handling.label()))
            })
            .collect();
        format!("coverage: {}", parts.join("; "))
    }

    /// Every entry no rule classifies, largest first.
    fn unknown_entries(&self) -> Unknown<'_> {
        let mut rows: Vec<&Row> =
            self.rows.iter().filter(|row| matches!(row.kind, Kind::Unknown) && row.role == "raw").collect();
        let total = rows.len();
        let bytes = rows.iter().map(|row| row.size).sum();
        rows.sort_by(|left, right| {
            right.size.cmp(&left.size).then_with(|| (&left.source, left.slot).cmp(&(&right.source, right.slot)))
        });
        Unknown { total, bytes, rows }
    }
}

#[derive(Default)]
struct Tally {
    entries: usize,
    bytes: u64,
    files: usize,
    written: u64,
    state: &'static str,
}

struct Unknown<'a> {
    total: usize,
    bytes: u64,
    rows: Vec<&'a Row>,
}
