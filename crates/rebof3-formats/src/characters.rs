//! The base-stat records of the eight playable characters, in `ETC/START.EMI` entry 8.
//!
//! Each record is [`RECORD_BYTES`] long and begins with the character's name, [`NAME_BYTES`] bytes in the name encoding
//! ([`crate::text::codec`]). The other stats are not typed yet, so a record's remaining bytes are written as hex. The
//! layout is in `wiki: data/records#character-and-master-data` ("Base stats (164 bytes)").

use core::fmt::Write as _;

use crate::error::{Error, Result};
use rebof3_emi::text::codec::{name_field, name_text};

/// Playable characters, one base-stat record each.
pub const CHARACTERS: usize = 8;
/// Bytes in one base-stat record.
pub const RECORD_BYTES: usize = 0xA4;
/// Bytes in a record's name field.
pub const NAME_BYTES: usize = 5;
/// The payload offset of the first record in `ETC/START.EMI` entry 8.
pub const PAYLOAD_OFFSET: usize = 0x1A914;

/// One character: its name in the name encoding, and the record's bytes after the name, as hex.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Character {
    pub name: String,
    pub rest: String,
}

/// The characters of an entry payload, in table order.
pub fn parse(payload: &[u8]) -> Result<Vec<Character>> {
    Ok(table_bytes(payload)?.as_chunks::<RECORD_BYTES>().0.iter().map(|record| Character { name: name_text(&record[..NAME_BYTES]), rest: hex(&record[NAME_BYTES..]) }).collect())
}

/// The entry payload with its characters replaced by `characters`, which must be [`CHARACTERS`] records.
pub fn write(characters: &[Character], payload: &[u8]) -> Result<Vec<u8>> {
    table_bytes(payload)?;
    if characters.len() != CHARACTERS {
        return Err(Error::CharacterTable(format!("{} characters; the table holds {CHARACTERS}", characters.len())));
    }
    let mut table = Vec::with_capacity(CHARACTERS * RECORD_BYTES);
    for character in characters {
        table.extend(name_field(&character.name, NAME_BYTES)?);
        let rest = unhex(&character.rest)?;
        if rest.len() != RECORD_BYTES - NAME_BYTES {
            return Err(Error::CharacterTable(format!("{:?} has {} bytes after its name; a record has {}", character.name, rest.len(), RECORD_BYTES - NAME_BYTES)));
        }
        table.extend(rest);
    }
    let mut out = payload.to_vec();
    out[PAYLOAD_OFFSET..PAYLOAD_OFFSET + table.len()].copy_from_slice(&table);
    Ok(out)
}

/// The JSON of an entry's characters: an array of `{"name", "rest"}` records.
pub fn export(payload: &[u8]) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec_pretty(&parse(payload)?).expect("a character serializes"))
}

/// The entry payload with its characters replaced by the JSON that [`export`] wrote.
pub fn import(json: &[u8], payload: &[u8]) -> Result<Vec<u8>> {
    let characters: Vec<Character> = serde_json::from_slice(json).map_err(|error| Error::CharacterTable(format!("the characters JSON does not read: {error}")))?;
    write(&characters, payload)
}

/// The table's bytes in an entry payload. A payload too short for the table is an error.
fn table_bytes(payload: &[u8]) -> Result<&[u8]> {
    let end = PAYLOAD_OFFSET + CHARACTERS * RECORD_BYTES;
    payload.get(PAYLOAD_OFFSET..end).ok_or(Error::GameTableShort { table: "characters", bytes: payload.len(), needed: end })
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn unhex(text: &str) -> Result<Vec<u8>> {
    let not_hex = || Error::CharacterTable(format!("{text:?} is not an even run of hex digits"));
    if !text.len().is_multiple_of(2) {
        return Err(not_hex());
    }
    text.as_bytes().as_chunks::<2>().0.iter().map(|pair| std::str::from_utf8(pair).ok().and_then(|digits| u8::from_str_radix(digits, 16).ok()).ok_or_else(not_hex)).collect()
}
