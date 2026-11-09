//! The strings of a payload that is not a message table, as JSON.

use std::collections::BTreeMap;

use rebof3_emi::Entry;
use rebof3_emi::text::codec::{self, Encoding};

use crate::text::discover::{self, Found, Kind};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::text::source::{Origin, PayloadSource};

/// Whether an entry can hold text: graphics (type 3) and audio (types 6 to 10) hold none.
pub fn holds_text(entry: &Entry) -> bool {
    !matches!(entry.file_type, 3 | 6..=10)
}

/// The address a payload loads at, when it is a RAM payload (types 0 to 2) whose address lies in the RAM window.
pub fn load_address(entry: &Entry) -> Option<u32> {
    if matches!(entry.file_type, 0..=2) { ram_address(entry.ram_ptr) } else { None }
}

/// `address` when it lies in the RAM window that payloads load into, and `None` otherwise (0 is never an
pub fn ram_address(address: u32) -> Option<u32> {
    (0x8000_0000..0x8020_0000).contains(&address).then_some(address)
}

/// The JSON form: the payload's size, so an import refuses another payload, and the strings in offset order.
#[derive(Debug, Deserialize, Serialize)]
struct Document {
    size: usize,
    strings: Vec<Item>,
}

/// One string.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Item {
    #[serde(flatten)]
    pub origin: Origin,
    /// String length in bytes.
    pub length: usize,
    /// The field stride when the string is a row of a fixed-width table.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stride: Option<usize>,
    /// The stored bytes, as hex.
    #[serde(default)]
    pub bytes: String,
    /// The decoded text.
    pub text: String,
    /// `message`, `string-table` or `loose`.
    #[serde(default)]
    pub kind: String,
    /// A stable id: `message[<slot>]`, `<table>[<index>]` or `loose`.
    #[serde(default)]
    pub id: String,
    /// The known table's name, when this row is a record of one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
    /// The record's index inside its table.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
    /// How the string was found: `table header`, `known table` or `scan`.
    #[serde(default)]
    pub found: String,
}

impl Item {
    /// A known-table row: the field's bytes, its stride, and the table and record index it belongs to.
    pub fn known(origin: Origin, raw: &[u8], stride: usize, text: String, table: &str, index: usize) -> Result<Self> {
        if codec::encode(Encoding::Message, &text)? != raw {
            return Err(Error::Import(format!("the name at {:#x} does not encode to its bytes", origin.offset)));
        }
        Ok(Self {
            origin,
            length: raw.len(),
            stride: Some(stride),
            bytes: hex(raw),
            text,
            kind: "string-table".into(),
            id: format!("{table}[{index}]"),
            table: Some(table.into()),
            index: Some(index),
            found: "known table".into(),
        })
    }
}

/// The strings of `payload` as JSON.
pub fn export(payload: &[u8], load_address: Option<u32>) -> Result<Vec<u8>> {
    export_counted(payload, load_address, None, &[]).map(|(json, _)| json)
}

/// The payload's scanned strings as rows, with the fields of `exclude` left out.
pub fn rows(
    payload: &[u8],
    load_address: Option<u32>,
    source: Option<&PayloadSource>,
    exclude: &[(usize, usize)],
) -> Result<Vec<Item>> {
    if discover::table_records(payload).is_some() {
        return Err(Error::Import("this payload is a message table; export it with the message table format".into()));
    }
    let strings = disjoint(discover::strings(payload, load_address));
    let mut fields = BTreeMap::new();
    let mut items = Vec::new();
    for found in strings {
        if exclude.iter().any(|&(from, to)| found.start < to && from < found.end) {
            continue;
        }
        let row = match found.kind {
            Kind::Field { stride } => {
                let next = fields.entry(stride).or_insert(0usize);
                let index = *next;
                *next += 1;
                let table = format!("scan@0x{stride:x}");
                let id = format!("{table}[{index}]");
                FoundRow { kind: "string-table", table: Some(table), index: Some(index), id }
            }
            Kind::Record => FoundRow { kind: "loose", table: None, index: None, id: "loose".into() },
            Kind::Message => FoundRow { kind: "message", table: None, index: None, id: "message".into() },
        };
        items.push(item(payload, found, source, row)?);
    }
    items.sort_by_key(|item| item.origin.offset);
    Ok(items)
}

/// The strings of `payload` as JSON, and how many strings it lists.
pub fn export_counted(
    payload: &[u8],
    load_address: Option<u32>,
    source: Option<&PayloadSource>,
    exclude: &[(usize, usize)],
) -> Result<(Vec<u8>, usize)> {
    let items = rows(payload, load_address, source, exclude)?;
    let count = items.len();
    Ok((document(payload.len(), &items), count))
}

/// The JSON document of already found rows.
pub fn document(size: usize, items: &[Item]) -> Vec<u8> {
    let document = Document { size, strings: items.to_vec() };
    serde_json::to_vec_pretty(&document).expect("the strings serialize")
}

/// The strings that do not overlap.
pub fn disjoint(found: Vec<Found>) -> Vec<Found> {
    let (names, records): (Vec<Found>, Vec<Found>) =
        found.into_iter().partition(|string| !matches!(string.kind, Kind::Record));
    let mut kept: BTreeMap<usize, Found> = BTreeMap::new();
    for string in names.into_iter().chain(records) {
        let before = kept.range(..=string.start).next_back().is_some_and(|(_, other)| other.end > string.start);
        let after = kept.range(string.start + 1..).next().is_some_and(|(&start, _)| start < string.end);
        if !before && !after {
            kept.insert(string.start, string);
        }
    }
    kept.into_values().collect()
}

/// The row fields a scan assigns to one found string.
struct FoundRow {
    kind: &'static str,
    table: Option<String>,
    index: Option<usize>,
    id: String,
}

fn item(payload: &[u8], found: Found, source: Option<&PayloadSource>, row: FoundRow) -> Result<Item> {
    let raw = &payload[found.start..found.end];
    let text = codec::decode(Encoding::Message, raw);
    if codec::encode(Encoding::Message, &text)? != raw {
        return Err(Error::Import(format!("the string at {:#x} does not encode to its bytes", found.start)));
    }
    let stride = match found.kind {
        Kind::Field { stride } => Some(stride),
        Kind::Message | Kind::Record => None,
    };
    Ok(Item {
        origin: source.map_or_else(|| Origin::plain(found.start), |source| source.origin(found.start)),
        length: raw.len(),
        stride,
        bytes: hex(raw),
        text,
        kind: row.kind.to_string(),
        id: row.id,
        table: row.table,
        index: row.index,
        found: "scan".into(),
    })
}

/// The payload's bytes with each string's text encoded again. `payload` must be the bytes [`export`] read.
pub fn import(json: &[u8], payload: &[u8]) -> Result<Vec<u8>> {
    let document: Document = serde_json::from_slice(json)
        .map_err(|error| Error::Import(format!("the strings JSON does not read: {error}")))?;
    if document.size != payload.len() {
        return Err(Error::Import(format!(
            "the strings were exported from {} bytes, and the payload has {}",
            document.size,
            payload.len()
        )));
    }
    let mut out = Vec::with_capacity(payload.len());
    let mut cursor = 0;
    for string in &document.strings {
        let end =
            string.origin.offset.checked_add(string.length).filter(|&end| end <= payload.len()).ok_or_else(|| {
                Error::Import(format!(
                    "the string at {:#x} runs past the payload's {} bytes",
                    string.origin.offset,
                    payload.len()
                ))
            })?;
        if string.origin.offset < cursor {
            return Err(Error::Import(format!(
                "the string at {:#x} overlaps the string before it",
                string.origin.offset
            )));
        }
        let mut bytes = codec::encode(Encoding::Message, &string.text)
            .map_err(|error| Error::Import(format!("the string at {:#x}: {error}", string.origin.offset)))?;
        if string.stride.is_some() {
            if bytes.len() > string.length {
                return Err(Error::Import(format!(
                    "the name at {:#x} is {} bytes, and its field holds {}",
                    string.origin.offset,
                    bytes.len(),
                    string.length
                )));
            }
            bytes.resize(string.length, 0);
        }
        out.extend_from_slice(&payload[cursor..string.origin.offset]);
        out.extend_from_slice(&bytes);
        cursor = end;
    }
    out.extend_from_slice(&payload[cursor..]);
    Ok(out)
}

/// The bytes as lowercase hex.
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}
