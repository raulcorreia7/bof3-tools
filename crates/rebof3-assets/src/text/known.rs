//! The strings that a documented table places, rather than a scan: the fixed game tables of
//! `ETC/GAME.EMI` entry 0, the base-stat character names, and the enemy names of an area encounter
//! block.
//!
//! Each row carries its table and record index, so a string in the output can be read back as the
//! game data that owns it. The layouts are in `wiki: data/records#gameemi-fixed-tables`,
//! `wiki: data/records#character-and-master-data` and `wiki: formats/area#0x800e4000-encounters-and-enemy-records`.

use rebof3_emi::characters;
use rebof3_formats::Format;
use rebof3_formats::game_tables::{self, PAYLOAD_START, TABLES, Table};

use crate::text::source::PayloadSource;

/// One string a documented table places.
#[derive(Clone, Debug)]
pub struct Known {
    pub offset: usize,
    pub length: usize,
    pub stride: usize,
    pub text: String,
    pub table: &'static str,
    pub index: usize,
}

impl Known {
    /// The row's byte range in the payload.
    pub fn range(&self) -> (usize, usize) {
        (self.offset, self.offset + self.stride)
    }
}

/// Every string the payload's documented tables place.
pub fn strings(source: &PayloadSource, slot: usize, format: Format, payload: &[u8]) -> Vec<Known> {
    match format {
        Format::CodeImage if is_game(source, slot) => game_tables(payload),
        Format::CodeImage if is_character_table(source, slot) => characters(payload),
        Format::AreaEncounterTable => enemies(payload),
        _ => Vec::new(),
    }
}

/// The fixed tables of `ETC/GAME.EMI` entry 0 that hold names.
fn game_tables(payload: &[u8]) -> Vec<Known> {
    let mut known = Vec::new();
    for table in TABLES.iter().filter(|table| has_names(table)) {
        let Ok(records) = table.records(payload) else { continue };
        for (index, record) in records.iter().enumerate() {
            let field = &record[..game_tables::NAME_BYTES];
            let Some(length) = name_length(field) else { continue };
            known.push(Known {
                offset: table.archive_offset - PAYLOAD_START + index * table.record,
                length,
                stride: table.record,
                text: game_tables::name_text(field),
                table: table.name,
                index,
            });
        }
    }
    known
}

/// Whether the table's records begin with a name field. Shops and level growth hold no names.
fn has_names(table: &Table) -> bool {
    matches!(table.name, "items" | "vital-items" | "weapons" | "armor" | "accessories" | "abilities")
}

/// The base-stat character names in `ETC/START.EMI` entry 8 and its `ETC/STATUS.EMI` copy.
fn characters(payload: &[u8]) -> Vec<Known> {
    let Ok(parsed) = characters::parse(payload) else { return Vec::new() };
    parsed
        .iter()
        .enumerate()
        .filter_map(|(index, character)| {
            let offset = characters::PAYLOAD_OFFSET + index * characters::RECORD_BYTES;
            let field = payload.get(offset..offset + characters::NAME_BYTES)?;
            let length = name_length(field)?;
            Some(Known {
                offset,
                length,
                stride: characters::RECORD_BYTES,
                text: character.name.clone(),
                table: "characters",
                index,
            })
        })
        .collect()
}

/// The eight enemy names at the start of each `0x88`-byte enemy record.
fn enemies(payload: &[u8]) -> Vec<Known> {
    const NAMES_START: usize = 0x48;
    const RECORD: usize = 0x88;
    const NAME_BYTES: usize = 8;
    const COUNT: usize = 8;
    let mut known = Vec::new();
    for index in 0..COUNT {
        let offset = NAMES_START + index * RECORD;
        let Some(field) = payload.get(offset..offset + NAME_BYTES) else { break };
        let Some(length) = name_length(field) else { continue };
        known.push(Known {
            offset,
            length,
            stride: RECORD,
            text: game_tables::name_text(field),
            table: "enemies",
            index,
        });
    }
    known
}

/// The bytes of a name field up to its last non-zero byte, or `None` when it is empty.
fn name_length(field: &[u8]) -> Option<usize> {
    field.iter().rposition(|byte| *byte != 0).map(|last| last + 1)
}

fn is_game(source: &PayloadSource, slot: usize) -> bool {
    slot == 0 && source.source.ends_with("ETC/GAME.EMI")
}

fn is_character_table(source: &PayloadSource, slot: usize) -> bool {
    (slot == 8 && source.source.ends_with("ETC/START.EMI")) || (slot == 0 && source.source.ends_with("ETC/STATUS.EMI"))
}
