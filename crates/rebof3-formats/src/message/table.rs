//! Message tables: a `u16` offset table, then the messages it points at.
//!
//! `offset[0]` is the table's length in bytes, so the table holds `offset[0] / 2` slots. A slot holds the offset of
//! its message, and a message runs to the next distinct offset. The slots past the last message point at the end of
//! the table. The table is the table, then the messages in offset order, so the messages and the slot map rebuild it
//! exactly. [`parse`] refuses a layout that this rule does not describe.

use std::collections::BTreeMap;

use psx_formats::bytes::u16_le;
use serde::{Deserialize, Serialize};

use rebof3_emi::text::codec::{self, Encoding};

use crate::error::{Error, Result};
use crate::text::source::{Origin, PayloadSource};

/// The messages of a table, and the slot map.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Table {
    /// Each message's bytes, in offset order.
    pub messages: Vec<Vec<u8>>,
    /// For every slot, the index of its message, or `None` for a slot past the last message.
    pub slots: Vec<Option<usize>>,
}

/// One exported message: its place, the slots that reach it, its bytes and its text.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Message {
    #[serde(flatten)]
    pub origin: Origin,
    /// A stable id: `message[<first slot>]`.
    pub id: String,
    /// Every slot that points at this message, lowest first.
    #[serde(default)]
    pub slots: Vec<usize>,
    /// Message length in bytes.
    pub length: usize,
    /// The stored bytes, as hex.
    #[serde(default)]
    pub bytes: String,
    /// The decoded text.
    pub text: String,
    /// How the message was found: `table header`.
    #[serde(default)]
    pub found: String,
    /// The string's kind: `message`.
    #[serde(default)]
    pub kind: String,
}

/// The JSON form: the payload size, one row per message, and the slot map. A slot is `null` when it
/// points past the last message.
#[derive(Debug, Deserialize, Serialize)]
struct Document {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    size: Option<usize>,
    messages: Vec<WireMessage>,
    slots: Vec<Option<usize>>,
}

/// A message row, or a bare string from an older export.
#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
enum WireMessage {
    Text(String),
    Row(Box<Message>),
}

impl WireMessage {
    fn text(&self) -> &str {
        match self {
            Self::Text(text) => text,
            Self::Row(row) => &row.text,
        }
    }
}

/// Reads a message table's table and messages.
pub fn parse(payload: &[u8]) -> Result<Table> {
    let invalid = |why: &str| Error::Import(format!("not a message table: {why}"));
    if payload.len() < 2 {
        return Err(invalid("shorter than its offset table"));
    }
    let table_len = usize::from(u16_le(payload, 0));
    if table_len < 2 || table_len % 2 != 0 || table_len > payload.len() {
        return Err(invalid("the table length is zero, odd, or longer than the table"));
    }
    let offsets: Vec<usize> = (0..table_len / 2).map(|slot| usize::from(u16_le(payload, 2 * slot))).collect();
    let mut starts = offsets.clone();
    starts.sort_unstable();
    starts.dedup();
    let end = payload.len();
    if starts.first() != Some(&table_len) || starts.last().is_some_and(|&last| last > end) {
        return Err(invalid("a slot points into the table or past the table"));
    }
    let mut messages = Vec::with_capacity(starts.len());
    let mut message_at = BTreeMap::new();
    for (index, &start) in starts.iter().enumerate().filter(|(_, start)| **start < end) {
        let stop = starts.get(index + 1).copied().unwrap_or(end);
        message_at.insert(start, messages.len());
        messages.push(payload[start..stop].to_vec());
    }
    // A slot at the end of the table has no message, so `get` gives `None` for it.
    let slots = offsets.iter().map(|offset| message_at.get(offset).copied()).collect();
    Ok(Table { messages, slots })
}

/// Lays a table out again: the table, then the messages in order. The result is the bytes [`parse`] read.
pub fn build(table: &Table) -> Result<Vec<u8>> {
    if table.slots.is_empty() {
        return Err(Error::Import("a message table has no slots".into()));
    }
    let table_len = 2 * table.slots.len();
    let mut starts = Vec::with_capacity(table.messages.len());
    let mut at = table_len;
    for (index, message) in table.messages.iter().enumerate() {
        if message.is_empty() {
            return Err(Error::Import(format!("message {index} is empty; an empty message has no offset")));
        }
        starts.push(at);
        at += message.len();
    }
    let end = at;
    if end > usize::from(u16::MAX) {
        return Err(Error::Import(format!("a table of {end} bytes is past the 64 KiB an offset can name")));
    }
    let mut out = Vec::with_capacity(end);
    for slot in &table.slots {
        let offset = match slot {
            Some(index) => *starts.get(*index).ok_or_else(|| {
                Error::Import(format!("a slot names message {index}, and the table has {}", starts.len()))
            })?,
            None => end,
        };
        out.extend_from_slice(&(offset as u16).to_le_bytes());
    }
    for message in &table.messages {
        out.extend_from_slice(message);
    }
    Ok(out)
}

/// The most bytes a table may hold: it loads at `0x8001_0000` and the resource table starts at `0x8001_A000`
/// (`Message_GetText (exe/slus_004_22@0x801502D0)` reads the table and the table from those addresses), so the
/// table has `0xA000` bytes before it runs into the table.
pub const LIMIT: usize = 0xA000;

/// [`build`], refusing a table that no longer fits the space the game gives it, with the reason.
pub fn build_within_limit(table: &Table) -> Result<Vec<u8>> {
    let bytes = build(table)?;
    if bytes.len() > LIMIT {
        return Err(Error::Import(format!(
            "the table would be {} bytes, {} past the {LIMIT} bytes between its load address 0x80010000 and the \
             resource table at 0x8001A000",
            bytes.len(),
            bytes.len() - LIMIT
        )));
    }
    Ok(bytes)
}

/// The table as JSON: each message decoded with its control codes kept, and the slot map. Refuses a table that
/// [`build`] would not lay out to the same bytes.
pub fn export(payload: &[u8]) -> Result<Vec<u8>> {
    export_counted(payload, None).map(|(json, _, _)| json)
}

/// The table as JSON, how many messages it lists, and the rows it wrote.
pub fn export_counted(payload: &[u8], source: Option<&PayloadSource>) -> Result<(Vec<u8>, usize, Vec<Message>)> {
    let table = parse(payload)?;
    if build(&table)? != payload {
        return Err(Error::Import("the table does not lay out to its own bytes; it is not exported".into()));
    }
    let table_len = usize::from(u16_le(payload, 0));
    let mut starts: Vec<usize> = (0..table_len / 2).map(|slot| usize::from(u16_le(payload, 2 * slot))).collect();
    starts.sort_unstable();
    starts.dedup();
    starts.retain(|&start| start < payload.len());
    let mut rows = Vec::with_capacity(table.messages.len());
    for (index, raw) in table.messages.iter().enumerate() {
        let start = starts[index];
        let slots: Vec<usize> =
            table.slots.iter().enumerate().filter_map(|(slot, at)| (*at == Some(index)).then_some(slot)).collect();
        let id = match slots.first() {
            Some(slot) => format!("message[{slot}]"),
            None => "message[?]".to_string(),
        };
        rows.push(Message {
            origin: source.map_or_else(|| Origin::plain(start), |source| source.origin(start)),
            id,
            slots,
            length: raw.len(),
            bytes: hex(raw),
            text: codec::decode(Encoding::Message, raw),
            found: "table header".into(),
            kind: "message".into(),
        });
    }
    let document = Document {
        size: Some(payload.len()),
        messages: rows.iter().cloned().map(|row| WireMessage::Row(Box::new(row))).collect(),
        slots: table.slots,
    };
    let count = rows.len();
    Ok((serde_json::to_vec_pretty(&document).expect("a message table serializes"), count, rows))
}

/// The table bytes that a JSON document from [`export`] stands for, with each message encoded again.
pub fn import(json: &[u8]) -> Result<Vec<u8>> {
    let document: Document = serde_json::from_slice(json)
        .map_err(|error| Error::Import(format!("the message table JSON does not read: {error}")))?;
    let mut messages = Vec::with_capacity(document.messages.len());
    for (index, message) in document.messages.iter().enumerate() {
        let bytes = codec::encode(Encoding::Message, message.text())
            .map_err(|error| Error::Import(format!("message {index}: {error}")))?;
        messages.push(bytes);
    }
    build(&Table { messages, slots: document.slots })
}

/// The bytes as lowercase hex.
fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}
