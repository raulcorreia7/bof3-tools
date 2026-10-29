//! Where the game's text sits in a payload: the scan the deleted Python text tool ran, ported so
//! that the Rust tools find the same strings with the same spans.
//!
//! A message bank holds its own offset table, and its records are its strings ([`bank_records`]). Any other payload
//! holds two kinds of string. A fixed-width name table holds names at a stride, found by the lattice search. Other
//! text is a record that ends at its terminator, or at the pointer table that reaches it ([`strings`]). The rules
//! and the evidence for them are in `wiki: game/text`.

use std::collections::{BTreeMap, BTreeSet};

use super::codec::Encoding;

/// Fewest bytes a record holds before it can be text.
const MIN_LENGTH: usize = 3;
/// Fewest records that keep one stride before they form a table.
const MIN_STRIDE_RUN: usize = 4;
/// Largest stride a table can have, in bytes.
const MAX_STRIDE: usize = 0x80;
/// Smallest stride a table can have, in bytes.
const MIN_FIELD_WIDTH: usize = 5;
/// Percentage of the observed record starts a table must explain.
const TABLE_COVERAGE: usize = 75;
/// A gap of this many strides between two records starts a new segment of the same table.
const SEGMENT_GAP: usize = 8;
/// Fewest letters a name field holds.
const MIN_LETTERS_FIELD: usize = 2;
/// A name that fills its field, with no terminator, must be at least this long.
const MIN_FULL_NAME: usize = 8;
/// Fewest printable bytes a loose record holds.
const MIN_LOOSE_LENGTH: usize = 6;
/// Longest record the scan accepts, in bytes.
const MAX_RECORD: usize = 0x1000;
/// Fewest letters a loose record holds.
const MIN_LETTERS_RUN: usize = 5;
/// Percentage of a loose record's printable bytes that are letters.
const LETTER_PERCENT: usize = 55;
/// Bytes below this value are control codes, never the start of text in a run.
const CONTROL_LIMIT: u8 = 0x20;
/// Records of a message bank that decide whether the bank holds text.
const BANK_SAMPLE: usize = 64;
/// Consecutive image pointers that make a pointer table.
const POINTER_WORDS: usize = 2;

/// What kind of string a [`Found`] is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    /// A record of a message bank, delimited by the bank's offset table.
    Message,
    /// A name in a fixed-width table whose records are `stride` bytes apart.
    Field { stride: usize },
    /// A record the scan found: it ends at its terminator, or where its pointer table begins.
    Record,
}

/// One string of a payload. Its bytes are `payload[start..end]`, its terminator included when it has one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Found {
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
}

/// The records of a message bank that holds text, or `None` for any other payload and for a bank whose records
/// read as code. Records that are shorter than [`MIN_LENGTH`] are left out.
pub fn bank_records(payload: &[u8]) -> Option<Vec<Found>> {
    let offsets = bank_slots(payload)?;
    if !bank_holds_text(payload, &offsets) {
        return None;
    }
    let bounds = distinct_sorted(&offsets);
    let mut found = Vec::new();
    for (index, &start) in bounds.iter().enumerate() {
        if start >= payload.len() {
            continue;
        }
        let stop = bounds.get(index + 1).copied().unwrap_or(payload.len());
        let span = &payload[start..stop];
        if span.first() == Some(&0) {
            continue;
        }
        let length = record_end(span);
        let terminated = span[..length].last() == Some(&0);
        if length - usize::from(terminated) < MIN_LENGTH {
            continue;
        }
        found.push(Found { start, end: start + length, kind: Kind::Message });
    }
    Some(found)
}

/// The strings of a payload that is not a message bank, in offset order. `base` is the address the payload loads
/// at, which the pointer-table test needs; `None` when the payload has none.
pub fn strings(payload: &[u8], base: Option<u32>) -> Vec<Found> {
    let runs = scan_runs(payload);
    let fields = lattice_fields(payload, &runs);
    let ranges: Vec<(usize, usize)> = fields.iter().map(|(&start, &(_, stride))| (start, start + stride)).collect();
    let mut found = Vec::new();
    for (start, end) in scan_records(payload, base, MAX_RECORD) {
        if fields.contains_key(&start) || ranges.iter().any(|&(from, to)| from < start && start < to) {
            continue;
        }
        if is_text_record(&payload[start..end]) {
            found.push(Found { start, end, kind: Kind::Record });
        }
    }
    for (&start, &(length, stride)) in &fields {
        found.push(Found { start, end: start + length, kind: Kind::Field { stride } });
    }
    found.sort_by_key(|string| string.start);
    found
}

/// Whether one message byte is a character: the glyphs of the message encoding.
fn drawable(byte: u8) -> bool {
    Encoding::Message.glyph(byte).is_some()
}

/// Whether one byte is a control code that a record may hold without its operand.
fn control(byte: u8) -> bool {
    matches!(byte, 0x01..=0x1F | 0x90)
}

/// Whether one byte is a control code with an operand byte after it.
fn operand(byte: u8) -> bool {
    Encoding::Message.operands().iter().any(|(code, _)| *code == byte)
}

/// The bytes a name field and a run of text may hold. The plus sign is left out.
fn printable(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, 0x3A..=0x3E | 0x5C | 0x5D | 0x8D..=0x8F | 0xFF)
}

fn distinct_sorted(values: &[usize]) -> Vec<usize> {
    values.iter().copied().collect::<BTreeSet<_>>().into_iter().collect()
}

/// The offset table of a message bank: its first word is the table's length in bytes, and every entry points
/// inside the payload, in order. `None` when the payload does not start with such a table.
fn bank_slots(payload: &[u8]) -> Option<Vec<usize>> {
    if payload.len() < 8 {
        return None;
    }
    let first = usize::from(u16::from_le_bytes([payload[0], payload[1]]));
    if first < 4 || first % 2 != 0 || first > payload.len() {
        return None;
    }
    let offsets: Vec<usize> =
        payload[..first].as_chunks::<2>().0.iter().map(|pair| usize::from(u16::from_le_bytes(*pair))).collect();
    if offsets[0] != first || offsets.iter().any(|&offset| offset < first || offset > payload.len()) {
        return None;
    }
    if offsets.windows(2).any(|pair| pair[0] > pair[1]) {
        return None;
    }
    Some(offsets)
}

/// Whether at least half of a bank's first records read as text.
fn bank_holds_text(payload: &[u8], offsets: &[usize]) -> bool {
    let bounds = distinct_sorted(offsets);
    let records: Vec<&[u8]> = (0..bounds.len().min(BANK_SAMPLE))
        .filter(|&index| bounds[index] < payload.len())
        .map(|index| {
            let stop = bounds.get(index + 1).copied().unwrap_or(payload.len());
            &payload[bounds[index]..stop]
        })
        .collect();
    if records.is_empty() {
        return false;
    }
    let good = records.iter().filter(|record| record_is_text(record)).count();
    good * 2 >= records.len()
}

/// Whether one bank record reads as game text rather than code bytes.
fn record_is_text(record: &[u8]) -> bool {
    let end = record.iter().rposition(|byte| *byte != 0).map_or(0, |last| last + 1);
    let body = &record[..end];
    if body.len() < MIN_LENGTH {
        return false;
    }
    let letters = body.iter().filter(|byte| byte.is_ascii_alphabetic()).count();
    let fills = body.iter().filter(|byte| **byte == 0xFF).count();
    (letters >= 4 && letters * 100 >= 40 * body.len()) || (letters >= 3 && fills >= 1)
}

/// The length of a bank record: up to and including its terminator, or the whole span when it has none. An
/// operand may be zero, so a control code and its operand are skipped together.
fn record_end(span: &[u8]) -> usize {
    let mut index = 0;
    while index < span.len() {
        let byte = span[index];
        if operand(byte) && index + 1 < span.len() {
            index += 2;
            continue;
        }
        if byte == 0 {
            return index + 1;
        }
        index += 1;
    }
    span.len()
}

/// The runs of printable bytes that end at a zero, with at least one letter: `(start, terminator)`.
fn scan_runs(payload: &[u8]) -> Vec<(usize, usize)> {
    let size = payload.len();
    let mut spans = Vec::new();
    let mut i = 0;
    while i < size {
        if payload[i] < CONTROL_LIMIT || !printable(payload[i]) {
            i += 1;
            continue;
        }
        let mut j = i;
        let mut letters = 0;
        while j < size && payload[j] >= CONTROL_LIMIT && printable(payload[j]) {
            if payload[j].is_ascii_alphabetic() {
                letters += 1;
            }
            j += 1;
        }
        if letters >= 1 && j - i >= MIN_LENGTH && j < size && payload[j] == 0 {
            spans.push((i, j));
            i = j + 1;
        } else {
            i = j;
        }
    }
    spans
}

/// Whether two consecutive words at `position` (or at the next word boundary) point inside the payload, which is
/// what the pointer table of a string without a terminator looks like.
fn pointer_table_at(payload: &[u8], position: usize, base: Option<u32>) -> bool {
    let Some(base) = base.map(u64::from) else {
        return false;
    };
    if position >= payload.len() {
        return false;
    }
    let limit = base + payload.len() as u64;
    [position, (position + 3) & !3].into_iter().any(|start| {
        let mut hits = 0;
        let mut offset = start;
        while offset + 4 <= payload.len() && hits < POINTER_WORDS {
            let word =
                u32::from_le_bytes([payload[offset], payload[offset + 1], payload[offset + 2], payload[offset + 3]]);
            if !(base <= u64::from(word) && u64::from(word) < limit) {
                break;
            }
            hits += 1;
            offset += 4;
        }
        hits >= POINTER_WORDS
    })
}

/// The records of a payload that begin with a byte the game can draw or run, as `(start, end)` with the terminator
/// included when there is one. A record that holds a byte the game cannot run ends the candidate.
fn scan_records(payload: &[u8], base: Option<u32>, limit: usize) -> Vec<(usize, usize)> {
    let size = payload.len();
    let mut spans = Vec::new();
    let mut i = 0;
    while i < size {
        let first = payload[i];
        if first == 0 || !(drawable(first) || control(first) || operand(first)) {
            i += 1;
            continue;
        }
        let mut j = i;
        let mut letters = 0;
        let mut aborted = false;
        let mut terminated = false;
        while j < size {
            let byte = payload[j];
            if byte == 0 {
                terminated = true;
                break;
            }
            if j % 4 == 0 && pointer_table_at(payload, j, base) {
                break;
            }
            if operand(byte) && j + 1 < size {
                j += 2;
                continue;
            }
            if !drawable(byte) && !control(byte) {
                aborted = true;
                break;
            }
            if byte.is_ascii_alphabetic() {
                letters += 1;
            }
            j += 1;
        }
        if !aborted && letters >= 1 && j - i <= limit {
            spans.push((i, if terminated { j + 1 } else { j }));
        }
        i = if j > i { j + 1 } else { i + 1 };
    }
    spans
}

/// Whether a record reads as text: its codes are few, it has printable bytes and enough letters.
fn is_text_record(raw: &[u8]) -> bool {
    let body = if raw.last() == Some(&0) { &raw[..raw.len() - 1] } else { raw };
    let mut codes = 0;
    let mut index = 0;
    while index < body.len() {
        let byte = body[index];
        if drawable(byte) {
            index += 1;
        } else if operand(byte) {
            index += 2;
            codes += 1;
        } else if control(byte) {
            index += 1;
            codes += 1;
        } else {
            return false;
        }
    }
    if codes * 4 > body.len() {
        return false;
    }
    let letters = raw.iter().filter(|byte| byte.is_ascii_alphabetic()).count();
    let printable_bytes = raw.iter().filter(|byte| drawable(**byte)).count();
    printable_bytes >= MIN_LOOSE_LENGTH
        && letters >= MIN_LETTERS_RUN
        && letters * 100 >= LETTER_PERCENT * printable_bytes.max(1)
}

/// The fixed-width name fields: `start -> (length, stride)`, where `length` is the name's bytes in the field.
fn lattice_fields(payload: &[u8], runs: &[(usize, usize)]) -> BTreeMap<usize, (usize, usize)> {
    let starts: Vec<usize> = runs.iter().map(|&(start, _)| start).collect();
    let mut fields = BTreeMap::new();
    let mut claimed = BTreeSet::new();
    for (stride, _coverage, positions) in field_chains(payload, &starts) {
        let fresh: Vec<usize> = positions.iter().copied().filter(|position| !claimed.contains(position)).collect();
        if fresh.len() < MIN_STRIDE_RUN {
            continue;
        }
        for position in fresh {
            if let Some(length) = field_at(payload, position, stride) {
                fields.insert(position, (length, stride));
            }
        }
        claimed.extend(positions);
    }
    fields
}

/// The candidate tables as `(stride, coverage, positions)`, best first. A table's stride is a difference between
/// two record starts, and a candidate explains the starts that fall on its lattice.
fn field_chains(payload: &[u8], starts: &[usize]) -> Vec<(usize, usize, Vec<usize>)> {
    let span_starts = distinct_sorted(starts);
    // Most seen first, and ties in the order first seen, as `Counter.most_common` lists them.
    let mut counts: Vec<(usize, usize)> = Vec::new();
    for pair in span_starts.windows(2) {
        let delta = pair[1] - pair[0];
        if (MIN_FIELD_WIDTH..=MAX_STRIDE).contains(&delta) {
            match counts.iter_mut().find(|(stride, _)| *stride == delta) {
                Some(entry) => entry.1 += 1,
                None => counts.push((delta, 1)),
            }
        }
    }
    counts.sort_by_key(|&(_, count)| std::cmp::Reverse(count));
    let start_set: BTreeSet<usize> = span_starts.iter().copied().collect();
    let mut candidates = Vec::new();
    for (stride, _) in counts {
        let anchors = span_starts.iter().copied().filter(|&position| {
            start_set.contains(&(position + stride))
                || position.checked_sub(stride).is_some_and(|before| start_set.contains(&before))
                || (position > 0 && payload[position - 1] == 0)
        });
        let mut phases: Vec<(usize, Vec<usize>)> = Vec::new();
        for position in anchors {
            let phase = position % stride;
            match phases.iter_mut().find(|(key, _)| *key == phase) {
                Some((_, members)) => members.push(position),
                None => phases.push((phase, vec![position])),
            }
        }
        for (_, members) in phases {
            for segment in segments(&members, stride) {
                if segment.len() < MIN_STRIDE_RUN {
                    continue;
                }
                let (table_start, table_end) = (segment[0], segment[segment.len() - 1]);
                let positions: Vec<usize> = (table_start..=table_end)
                    .step_by(stride)
                    .filter(|&position| field_at(payload, position, stride).is_some())
                    .collect();
                let observed: Vec<usize> = span_starts
                    .iter()
                    .copied()
                    .filter(|&position| table_start <= position && position <= table_end)
                    .collect();
                if observed.len() < MIN_STRIDE_RUN || positions.len() < MIN_STRIDE_RUN {
                    continue;
                }
                let lattice: BTreeSet<usize> = positions.iter().copied().collect();
                let explained = observed.iter().filter(|position| lattice.contains(position)).count();
                let coverage = explained * 100 / observed.len();
                if coverage >= TABLE_COVERAGE {
                    candidates.push((stride, coverage, positions));
                }
            }
        }
    }
    candidates.sort_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(&right.0)));
    candidates
}

/// Splits record starts into runs that are not far apart.
fn segments(members: &[usize], stride: usize) -> Vec<Vec<usize>> {
    let mut segments = Vec::new();
    let mut current: Vec<usize> = Vec::new();
    for &position in members {
        if current.last().is_some_and(|&last| position - last > stride * SEGMENT_GAP) {
            segments.push(std::mem::take(&mut current));
        }
        current.push(position);
    }
    if !current.is_empty() {
        segments.push(current);
    }
    segments
}

/// The length of the name in a field at `start`, or `None` when the field does not hold a name. The name is the
/// run of printable bytes from the field's start, and it ends at a zero (included) or fills the field.
fn field_at(payload: &[u8], start: usize, stride: usize) -> Option<usize> {
    if start >= payload.len() {
        return None;
    }
    let field = &payload[start..(start + stride).min(payload.len())];
    if field.len() < MIN_LENGTH || !field[0].is_ascii_alphabetic() {
        return None;
    }
    let mut length = 0;
    while length < field.len() && field[length] >= CONTROL_LIMIT && printable(field[length]) {
        length += 1;
    }
    if length < MIN_LENGTH
        || field[..length].iter().filter(|byte| byte.is_ascii_alphabetic()).count() < MIN_LETTERS_FIELD
    {
        return None;
    }
    if length < field.len() && field[length] == 0 {
        return Some(length + 1);
    }
    (length >= MIN_FULL_NAME).then_some(length)
}
