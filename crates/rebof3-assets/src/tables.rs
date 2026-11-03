//! The fixed data tables of `ETC/GAME.EMI` entry 0 as JSON: items, vital items, weapons, armor,
//! accessories, shops, abilities and level growth.
//!
//! The typed records live in `rebof3_emi::game_tables`, which this module re-exports. The JSON keeps each
//! record's fields by name. A name is text in the name encoding of `rebof3_emi::text::codec`: its codes are `{TAG}`
//! markers, and a byte the encoding has no character for is `{RAW:HH}`. The zeros that pad the field are not written.
//!
//! # Cross-reference
//!
//! Every record here has one C counterpart, and both name the same fields
//! (`wiki: data/records#gameemi-fixed-tables`):
//!
//! | Table | C record (`decomp/include`) |
//! | --- | --- |
//! | items | `game/item.h` `ItemObject` |
//! | vital items | `game/item.h` `VitalItem` |
//! | weapons | `game/item.h` `WeaponObject` |
//! | armor | `game/item.h` `ArmorObject` |
//! | accessories | `game/item.h` `AccessoryObject` |
//! | shops | `game/item.h` `ShopStock`, `ShopItemRef` |
//! | abilities | `game/ability.h` `Ability` |
//! | level growth | `game/character.h` `LevelObject` |
//!
//! The full map, with the field names that differ, is
//! `wiki: formats/conversion#c-records-rust-decoders-and-the-layout`: the level-growth record's second
//! ability byte is `extra_ability` in C and `ability_candidate` here, and the Rust `Growth` unpacks the two
//! packed stat bytes the C record keeps in `power_defense` and `agility_intellect`. The description resource
//! id that the C records hold has no field here.

use rebof3_emi::game_tables::{GameTables, TABLES};

pub use rebof3_emi::game_tables::{Table, name_text};

use crate::Result;

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

/// The eight playable characters of `ETC/START.EMI` entry 8 as JSON: each name, and the rest of its base-stat record as
/// hex. `payload` is the entry's stored bytes.
pub fn characters(payload: &[u8]) -> Result<Vec<u8>> {
    Ok(rebof3_emi::characters::export(payload)?)
}

/// The JSON of one area's encounter block: its eight encounters and eight enemy records. The enemy
/// indexes of an encounter name the records in `enemies`.
pub fn encounters(payload: &[u8]) -> Result<Vec<u8>> {
    Ok(json(&rebof3_emi::encounter::EncounterBlock::parse(payload)?))
}

/// Serializes a value the records define. None of these values can fail to serialize.
fn json<T: serde::Serialize>(value: &T) -> Vec<u8> {
    serde_json::to_vec_pretty(value).expect("a game table record serializes")
}
