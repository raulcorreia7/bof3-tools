//! The game tables as open files: one JSON per table out, and a table's JSON back over its stored bytes.

use super::{GameTables, PAYLOAD_START, TABLES, Table};
use crate::error::{Error, Result};

/// The JSON file of each table, in table order: the table's name and its records as an array.
pub fn files(payload: &[u8]) -> Result<Vec<(&'static str, Vec<u8>)>> {
    let tables = GameTables::parse(payload)?;
    Ok(vec![
        ("items", json(&tables.items)),
        ("vital-items", json(&tables.vital_items)),
        ("weapons", json(&tables.weapons)),
        ("armor", json(&tables.armor)),
        ("accessories", json(&tables.accessories)),
        ("shops", json(&tables.shops)),
        ("abilities", json(&tables.abilities)),
        ("level-growth", json(&tables.level_growth)),
    ])
}

/// Each table's stored bytes, in table order: the table, and its records exactly as the entry holds them.
pub fn stored(payload: &[u8]) -> Result<Vec<(Table, &[u8])>> {
    let mut found = Vec::with_capacity(TABLES.len());
    for table in TABLES {
        found.push((table, table.bytes(payload)?));
    }
    Ok(found)
}

/// One JSON object with every table as a member named after it, for a reader that loads them together.
pub fn document(payload: &[u8]) -> Result<Vec<u8>> {
    let tables = GameTables::parse(payload)?;
    Ok(json(&tables))
}

/// One fixed table of `ETC/GAME.EMI` entry 0, from its JSON records (`<name>.json`, an array of the records).
pub fn import(table: &str, json: &[u8], base: &[u8]) -> Result<Vec<u8>> {
    let descriptor = *TABLES
        .iter()
        .find(|descriptor| descriptor.name == table)
        .ok_or_else(|| Error::Import(format!("there is no table named {table:?}")))?;
    if base.len() != descriptor.count * descriptor.record {
        return Err(Error::Import(format!(
            "{table} is {} records of {} bytes; its stored bytes are {}",
            descriptor.count,
            descriptor.record,
            base.len()
        )));
    }
    match table {
        "items" => rebuild(descriptor, json, base, super::put_item),
        "vital-items" => rebuild(descriptor, json, base, super::put_vital_item),
        "weapons" => rebuild(descriptor, json, base, super::put_weapon),
        "armor" => rebuild(descriptor, json, base, super::put_armor),
        "accessories" => rebuild(descriptor, json, base, super::put_accessory),
        "shops" => rebuild(descriptor, json, base, super::put_shop),
        "abilities" => rebuild(descriptor, json, base, super::put_ability),
        "level-growth" => rebuild(descriptor, json, base, super::put_growth),
        _ => Err(Error::Import(format!("{table} has no writer"))),
    }
}

/// One fixed table of `ETC/GAME.EMI` entry 0, from its JSON records and the stored payload of that entry.
pub fn import_in_payload(table: &str, json: &[u8], payload: &[u8]) -> Result<Vec<u8>> {
    let descriptor = TABLES
        .iter()
        .find(|descriptor| descriptor.name == table)
        .ok_or_else(|| Error::Import(format!("there is no table named {table:?}")))?;
    let start = descriptor.archive_offset - PAYLOAD_START;
    let end = start + descriptor.count * descriptor.record;
    let base = payload.get(start..end).ok_or_else(|| Error::Import(format!("the payload ends before {table}")))?;
    import(table, json, base)
}

/// Parses a table's JSON records and writes each one over its base record.
fn rebuild<T: serde::de::DeserializeOwned>(
    table: Table,
    json: &[u8],
    base: &[u8],
    put: fn(&mut [u8], &T) -> std::result::Result<(), String>,
) -> Result<Vec<u8>> {
    let records: Vec<T> =
        serde_json::from_slice(json).map_err(|error| Error::Import(format!("{} JSON: {error}", table.name)))?;
    if records.len() != table.count {
        return Err(Error::Import(format!(
            "{} has {} records; the table has {}",
            table.name,
            records.len(),
            table.count
        )));
    }
    let mut bytes = base.to_vec();
    for (record, value) in bytes.chunks_exact_mut(table.record).zip(&records) {
        put(record, value).map_err(|reason| Error::Import(format!("{} record: {reason}", table.name)))?;
    }
    Ok(bytes)
}

/// Serializes a value the records define. None of these values can fail to serialize.
fn json<T: serde::Serialize>(value: &T) -> Vec<u8> {
    serde_json::to_vec_pretty(value).expect("a game table record serializes")
}
