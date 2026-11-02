//! Format coverage scoreboard over the retail archives.
//!
//! One row per entry format: how many entries and bytes the retail archives
//! hold, what this crate can do with them (a decode → encode codec, a
//! documented layout without a codec, or nothing yet), and what the codec gate
//! observed. The `coverage` verb prints the scoreboard and writes the
//! `coverage.jsonl` table; the `retail_coverage` test asserts the pinned totals.
//!
//! Format selection reads the entry header — file type plus load address —
//! plus one payload property, an all-zero body, which is the reserved-slot
//! shape the asset census counts on its own. Names and addresses come from
//! `wiki: formats/area`, `wiki: formats/audio`,
//! `wiki: formats/graphics`, `wiki: data/text` and the `wiki: formats/assets` catalog.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::area::{map, scene};
use crate::cue;
use crate::emi::{Archive, Entry};
use crate::error::{Error, Result};
use crate::pack::{Payload, pack_payloads};
use psx_formats::clut;
use psx_formats::sep;
use psx_formats::vab::{self, Bank};
use psx_formats::vram::{self, Descriptor};

pub use crate::classify::{Format, State, classify};

/// One format's counts in the retail archives.
#[derive(Clone, Debug, Default)]
pub struct Row {
    /// Entries in this format.
    pub entries: usize,
    /// Their payload bytes.
    pub bytes: u64,
    /// Entries whose owning codec rebuilt the payload byte-exactly.
    pub covered_entries: usize,
    /// Their payload bytes.
    pub covered_bytes: u64,
    /// Entry type → count, for the coverage table's `entry_types` column.
    pub types: BTreeMap<u16, usize>,
}

/// The container round trip over every retail archive.
#[derive(Clone, Debug, Default)]
pub struct Container {
    pub archives: usize,
    pub entries: usize,
    pub bytes: u64,
    /// One line per archive that did not repack byte-exactly.
    pub failures: Vec<String>,
}

/// The scoreboard: one row per format, the container result and every failure.
#[derive(Clone, Debug, Default)]
pub struct Report {
    rows: Vec<Row>,
    pub container: Container,
    /// Entries whose codec did not rebuild their payload, one line each.
    pub failures: Vec<String>,
    /// Load addresses of unclassified entries → (entries, bytes).
    residual: BTreeMap<u32, (usize, u64)>,
}

impl Report {
    fn new() -> Self {
        Self {
            rows: vec![Row::default(); Format::ALL.len()],
            container: Container::default(),
            failures: Vec::new(),
            residual: BTreeMap::new(),
        }
    }

    /// The row for one format.
    pub fn row(&self, format: Format) -> &Row {
        &self.rows[format as usize]
    }

    /// Entries across the retail archives.
    pub fn entries(&self) -> usize {
        self.rows.iter().map(|row| row.entries).sum()
    }

    /// Payload bytes across the retail archives.
    pub fn bytes(&self) -> u64 {
        self.rows.iter().map(|row| row.bytes).sum()
    }

    /// Entries in one state.
    pub fn state_entries(&self, state: State) -> usize {
        self.by_state(state, |row| row.entries)
    }

    /// Payload bytes in one state.
    pub fn state_bytes(&self, state: State) -> u64 {
        self.by_state(state, |row| row.bytes)
    }

    /// Entries a codec rebuilt byte-exactly.
    pub fn covered_entries(&self) -> usize {
        self.rows.iter().map(|row| row.covered_entries).sum()
    }

    /// Payload bytes a codec rebuilt byte-exactly.
    pub fn covered_bytes(&self) -> u64 {
        self.rows.iter().map(|row| row.covered_bytes).sum()
    }

    fn by_state<T: std::iter::Sum>(&self, state: State, count: fn(&Row) -> T) -> T {
        Format::ALL.iter().filter(|format| format.state() == state).map(|format| count(self.row(*format))).sum()
    }

    /// Every failure in the run: codec failures, then container failures.
    pub fn all_failures(&self) -> Vec<String> {
        let mut failures = self.failures.clone();
        failures.extend(self.container.failures.iter().cloned());
        failures
    }

    /// The scoreboard as text.
    pub fn table(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "{:<48} {:>7} {:>12} {:>10} {:>10}\n",
            "format", "entries", "bytes", "state", "round-trip"
        ));
        for format in Format::ALL {
            let row = self.row(format);
            if row.entries == 0 {
                continue;
            }
            out.push_str(&format!(
                "{:<48} {:>7} {:>12} {:>10} {:>10}\n",
                format.name(),
                row.entries,
                row.bytes,
                format.state().label(),
                round_trip_cell(format, row),
            ));
        }
        out.push_str(&format!("{:<48} {:>7} {:>12}\n", "TOTAL", self.entries(), self.bytes()));
        out.push_str(&format!(
            "{:<48} {:>7} {:>12} {:>10}\n",
            "codec-covered",
            self.covered_entries(),
            self.covered_bytes(),
            percent(self.covered_bytes(), self.bytes()),
        ));
        for state in [State::Specified, State::Unknown, State::Code, State::Reserve] {
            out.push_str(&format!(
                "{:<48} {:>7} {:>12} {:>10}\n",
                state.label(),
                self.state_entries(state),
                self.state_bytes(state),
                percent(self.state_bytes(state), self.bytes()),
            ));
        }
        for (address, (entries, bytes)) in &self.residual {
            out.push_str(&format!(
                "{:<48} {:>7} {:>12}\n",
                format!("  unclassified at {address:#010x}"),
                entries,
                bytes
            ));
        }
        out.push_str(&format!(
            "container: {}/{} archives byte-identical, {} entries, {} bytes\n",
            self.container.archives - self.container.failures.len(),
            self.container.archives,
            self.container.entries,
            self.container.bytes,
        ));
        out
    }

    /// The scoreboard as the `coverage.jsonl` table: one JSON object per
    /// format, then the total.
    pub fn jsonl(&self) -> String {
        let mut out = String::new();
        for format in Format::ALL {
            let row = self.row(format);
            if row.entries == 0 {
                continue;
            }
            let record = serde_json::json!({
                "format": format.name(),
                "entries": row.entries,
                "bytes": row.bytes,
                "entry_types": type_list(row),
                "spec_state": format.spec_state(),
                "decoder_state": format.codec_state(),
                "roundtrip_state": round_trip_state(format, row),
                "evidence": format.evidence(),
            });
            out.push_str(&format!("{record}\n"));
        }
        let total = serde_json::json!({"format": "TOTAL", "entries": self.entries(), "bytes": self.bytes()});
        out.push_str(&format!("{total}\n"));
        out
    }

    /// Counts one entry, runs its codec when the format has one, and records
    /// the result.
    fn record(&mut self, source: &str, index: usize, entry: &Entry, payload: &[u8], banks: &BankResults) {
        let format = classify(entry, payload);
        let row = &mut self.rows[format as usize];
        row.entries += 1;
        row.bytes += u64::from(entry.size);
        *row.types.entry(entry.file_type).or_default() += 1;
        if format == Format::Unclassified {
            let slot = self.residual.entry(entry.ram_ptr).or_default();
            slot.0 += 1;
            slot.1 += u64::from(entry.size);
        }

        let outcome = match format {
            Format::VramUpload => round_trip_upload(entry, payload),
            Format::ClutBank => round_trip_palette(payload),
            Format::VabHeader | Format::VabBody => banks.outcome(index),
            Format::SepPackage => round_trip_package(payload),
            Format::CueTable => round_trip_cues(payload),
            Format::AreaActorDefsTable => round_trip_scenes(payload),
            Format::AreaMapBundle => round_trip_map(payload),
            _ => return,
        };
        match outcome {
            Ok(()) => {
                let row = &mut self.rows[format as usize];
                row.covered_entries += 1;
                row.covered_bytes += u64::from(entry.size);
            }
            Err(reason) => self.failures.push(format!("{source}#{index}: {reason}")),
        }
    }
}

/// Every `.EMI` container under `root`, through subdirectories, sorted.
pub fn archives_under(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    collect_archives(root, &mut found);
    found.sort();
    found
}

fn collect_archives(root: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_archives(&path, found);
        } else if path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("EMI")) {
            found.push(path);
        }
    }
}

/// Checks the retail archives: every entry's format, its codec round trip, and
/// the container round trip of every archive.
pub fn check(root: &Path) -> Result<Report> {
    let archives = archives_under(root);
    if archives.is_empty() {
        return Err(Error::InvalidArchive("corpus holds no .EMI container"));
    }

    let mut report = Report::new();
    for path in &archives {
        let label = path.display().to_string();
        match fs::read(path) {
            Ok(bytes) => report.check_archive(&label, &bytes),
            Err(error) => report.failures.push(format!("{label}: {error}")),
        }
    }
    Ok(report)
}

/// Repacks every archive under `root` from its parsed table and compares the
/// bytes with the retail container.
pub fn container_round_trip(root: &Path) -> Result<Container> {
    let mut report = Report::new();
    for path in archives_under(root) {
        report.check_container(&path.display().to_string(), &fs::read(&path)?);
    }
    Ok(report.container)
}

impl Report {
    /// Checks one container held in memory: the format and codec round trip of
    /// every entry, then the container round trip. `label` names it in failures.
    pub fn check_archive(&mut self, label: &str, bytes: &[u8]) {
        let Some(archive) = self.check_container(label, bytes) else {
            return;
        };
        let banks = check_banks(&archive, bytes);
        for (index, entry) in archive.entries().iter().enumerate() {
            match archive.payload(bytes, index) {
                Ok(payload) => self.record(label, index, entry, payload, &banks),
                Err(error) => self.failures.push(format!("{label}#{index}: {error}")),
            }
        }
    }

    /// Repacks one container's payloads and compares the bytes with the retail
    /// container. Returns the parsed table when the container opens.
    fn check_container(&mut self, label: &str, bytes: &[u8]) -> Option<Archive> {
        self.container.archives += 1;
        let archive = match Archive::parse(bytes) {
            Ok(archive) => archive,
            Err(error) => {
                self.container.failures.push(format!("{label}: {error}"));
                return None;
            }
        };
        self.container.entries += archive.entries().len();
        self.container.bytes += archive.entries().iter().map(|entry| u64::from(entry.size)).sum::<u64>();

        let payloads: Result<Vec<Payload>> = archive
            .entries()
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                Ok(Payload {
                    bytes: archive.payload(bytes, index)?,
                    file_type: entry.file_type,
                    ram_ptr: entry.ram_ptr,
                })
            })
            .collect();
        let packed = payloads.and_then(|payloads| pack_payloads(archive.version(), &payloads));
        match packed {
            Ok(packed) => {
                if let Some(offset) = first_difference(bytes, &packed) {
                    self.container.failures.push(format!(
                        "{label}: differs at 0x{offset:X} (retail {} bytes, repacked {} bytes)",
                        bytes.len(),
                        packed.len()
                    ));
                }
            }
            Err(error) => self.container.failures.push(format!("{label}: pack failed: {error}")),
        }
        Some(archive)
    }
}

/// First offset where two byte runs differ, or `None` when they are equal.
pub fn first_difference(left: &[u8], right: &[u8]) -> Option<usize> {
    let common = left.len().min(right.len());
    for index in 0..common {
        if left[index] != right[index] {
            return Some(index);
        }
    }
    (left.len() != right.len()).then_some(common)
}

/// Round trip results of an archive's VAB banks, keyed by entry index: a body
/// is covered when the header that carries it rebuilds both byte runs.
#[derive(Default)]
struct BankResults {
    outcomes: BTreeMap<usize, std::result::Result<(), String>>,
}

impl BankResults {
    fn outcome(&self, index: usize) -> std::result::Result<(), String> {
        self.outcomes.get(&index).cloned().unwrap_or_else(|| Err("no VAB header carries this entry".into()))
    }
}

fn check_banks(archive: &Archive, container: &[u8]) -> BankResults {
    let mut results = BankResults::default();
    for (index, entry) in archive.entries().iter().enumerate() {
        if entry.file_type != vab::HEADER_TYPE {
            continue;
        }
        let outcome = round_trip_bank(archive, container, index);
        if let Ok(body) = crate::entry::vab_body_index(archive.entries(), index) {
            results.outcomes.insert(body, outcome.clone());
        }
        results.outcomes.insert(index, outcome);
    }
    results
}

fn round_trip_bank(archive: &Archive, container: &[u8], index: usize) -> std::result::Result<(), String> {
    let describe = |error: &dyn ToString| error.to_string();
    let header = archive.payload(container, index).map_err(|e| describe(&e))?;
    let body_index = crate::entry::vab_body_index(archive.entries(), index).map_err(|e| describe(&e))?;
    let body = archive.payload(container, body_index).map_err(|e| describe(&e))?;
    let bank = Bank::parse(header, body).map_err(|e| describe(&e))?;
    if bank.encode_header() != header {
        return Err("the encoded header is not the header".into());
    }
    if bank.body_len() != body.len() {
        return Err("the parsed body is not the body entry's length".into());
    }
    Ok(())
}

fn round_trip_upload(entry: &Entry, payload: &[u8]) -> std::result::Result<(), String> {
    let rectangles =
        vram::decode(payload, Descriptor::from_load_argument(entry.ram_ptr)).map_err(|error| error.to_string())?;
    difference(&vram::encode(&rectangles), payload)
}

fn round_trip_palette(payload: &[u8]) -> std::result::Result<(), String> {
    let palette = clut::Palette::decode(payload).map_err(|error| error.to_string())?;
    difference(&palette.encode(), payload)
}

fn round_trip_package(payload: &[u8]) -> std::result::Result<(), String> {
    let package = sep::Package::parse(payload).map_err(|error| error.to_string())?;
    difference(&package.encode(), payload)
}

fn round_trip_cues(payload: &[u8]) -> std::result::Result<(), String> {
    let table = cue::CueTable::decode(payload).map_err(|error| error.to_string())?;
    difference(&table.encode(), payload)
}

fn round_trip_scenes(payload: &[u8]) -> std::result::Result<(), String> {
    let table = scene::SceneTable::decode(payload).map_err(|error| error.to_string())?;
    difference(&table.encode(), payload)
}

fn round_trip_map(payload: &[u8]) -> std::result::Result<(), String> {
    let bundle = map::MapBundle::decode(payload).map_err(|error| error.to_string())?;
    difference(&bundle.encode(), payload)
}

/// `Err` with the first difference when a codec's output is not its input.
fn difference(encoded: &[u8], payload: &[u8]) -> std::result::Result<(), String> {
    match first_difference(encoded, payload) {
        None => Ok(()),
        Some(offset) => Err(format!(
            "the encoded payload is not the payload ({} bytes against {}, first difference at \
             0x{offset:X})",
            encoded.len(),
            payload.len()
        )),
    }
}

fn round_trip_cell(format: Format, row: &Row) -> String {
    if format.state() != State::Codec {
        return "-".into();
    }
    format!("{}/{}", row.covered_entries, row.entries)
}

fn round_trip_state(format: Format, row: &Row) -> String {
    match format.state() {
        State::Codec if row.covered_entries == row.entries => {
            format!("byte-exact ({}/{})", row.covered_entries, row.entries)
        }
        State::Codec => format!("incomplete ({}/{})", row.covered_entries, row.entries),
        State::Reserve => "n/a (all-zero)".into(),
        State::Code => "n/a (code)".into(),
        _ => "container-only".into(),
    }
}

fn type_list(row: &Row) -> String {
    row.types.iter().map(|(file_type, count)| format!("{file_type}:{count}")).collect::<Vec<String>>().join(",")
}

fn percent(part: u64, whole: u64) -> String {
    if whole == 0 {
        return "-".into();
    }
    format!("{:.1}%", 100.0 * part as f64 / whole as f64)
}
