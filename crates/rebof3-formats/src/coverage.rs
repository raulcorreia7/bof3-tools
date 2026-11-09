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
use std::path::Path;

use rebof3_emi::verify::Container;
use rebof3_emi::{Archive, Entry, Error, Result};

use crate::registry::{Format, RoundTrip, State, classify};
use crate::vab;

pub use rebof3_emi::archives_under;
pub use rebof3_emi::verify::{container_round_trip, first_difference};

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

        let outcome = match format.describe().round_trip {
            RoundTrip::Payload(check) => check(entry, payload),
            RoundTrip::Bank => banks.outcome(index),
            RoundTrip::None => return,
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

impl Report {
    /// Checks one container held in memory: the format and codec round trip of
    /// every entry, then the container round trip. `label` names it in failures.
    pub fn check_archive(&mut self, label: &str, bytes: &[u8]) {
        let Some(archive) = self.container.check(label, bytes) else {
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
        if !vab::is_header(entry) {
            continue;
        }
        let outcome = round_trip_bank(archive, container, index);
        if let Ok(body) = rebof3_emi::entry::vab_body_index(archive.entries(), index) {
            results.outcomes.insert(body, outcome.clone());
        }
        results.outcomes.insert(index, outcome);
    }
    results
}

fn round_trip_bank(archive: &Archive, container: &[u8], index: usize) -> std::result::Result<(), String> {
    let describe = |error: &dyn ToString| error.to_string();
    let header = archive.payload(container, index).map_err(|e| describe(&e))?;
    let body_index = rebof3_emi::entry::vab_body_index(archive.entries(), index).map_err(|e| describe(&e))?;
    let body = archive.payload(container, body_index).map_err(|e| describe(&e))?;
    vab::round_trip(header, body)
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
