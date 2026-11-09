//! The encounter block of an area: eight encounters and eight enemy records (`0x800E4000`).
//!
//! The block is 1,160 bytes. Eight encounters of 9 bytes come first: the enemy indexes of the encounter
//! (`0xFF` is an empty slot) and its appearance rate. Eight enemies of 136 bytes (`0x88`) follow at
//! payload offset `0x48`. The layout is in `wiki: data/records#area-data` ("Enemies (136 bytes)")
//! and `wiki: formats/area#0x800e4000-encounters-and-enemy-records`; the picker is
//! `emi/etc/game/00@0x801C6A7C`.

use rebof3_emi::record::{bytes, u16_at};
use rebof3_emi::text::codec::{name_field, name_text};

use crate::error::{Error, Result};
use crate::registry::{Description, EMI_EXTENSION, Format, RoundTrip, Selector, State};

pub mod open;

/// The load address of the block.
const LOAD_ADDRESS: u32 = 0x800e_4000;

/// The block as a format-registry row.
pub static TABLE: Description = Description {
    format: Format::AreaEncounterTable,
    name: "area encounters and enemy records (0x800E4000)",
    state: State::Specified,
    spec_state: "documented",
    codec_state: "none",
    evidence: "wiki: formats/area (`0x800E4000`); include/game/enemy.h models it",
    extension: EMI_EXTENSION,
    selector: Selector::Load(&[LOAD_ADDRESS]),
    round_trip: RoundTrip::None,
    derive: None,
};

/// Bytes in the block.
pub const BLOCK_BYTES: usize = 0x488;
/// Encounter records, and the enemy indexes each one holds.
pub const ENCOUNTERS: usize = 8;
pub const ENCOUNTER_BYTES: usize = 9;
pub const ENCOUNTER_ENEMIES: usize = 8;
/// Enemy records, and the bytes of each.
pub const ENEMIES: usize = 8;
pub const ENEMY_BYTES: usize = 0x88;
/// The payload offset of the first enemy record.
pub const ENEMY_BASE: usize = ENCOUNTERS * ENCOUNTER_BYTES;
/// Bytes in an enemy's name.
pub const NAME_BYTES: usize = 8;

/// One encounter: the enemy indexes it draws and its appearance rate. `0xFF` is an empty slot.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, Eq, PartialEq)]
pub struct Encounter {
    pub enemies: [u8; ENCOUNTER_ENEMIES],
    pub appearance_rate: u8,
}

/// One item an enemy steals or drops: its index in the item type's table and the type.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, Eq, PartialEq)]
pub struct ItemRef {
    pub index: u8,
    pub item_type: u8,
}

/// One action block of an enemy: a condition byte, seven AI parameters and eight ability indexes. A
/// condition of `0x63` marks an unused block.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct ActionBlock {
    pub condition: u8,
    pub ai_params: [u8; 7],
    pub skills: [u8; 8],
}

/// One enemy record. The fields the decompilation has not named are left out.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct Enemy {
    #[serde(serialize_with = "serialize_name", deserialize_with = "deserialize_name")]
    pub name: [u8; NAME_BYTES],
    /// The unique enemy id; some enemies have more than one (`wiki: data/records#area-data`,
    /// "Enemy IDs").
    pub enemy_id: u16,
    pub choice_ai: u8,
    pub target_preference: u8,
    pub zenny: u16,
    pub exp: u16,
    pub level: u8,
    pub initial_skills: [u8; 8],
    pub hp: u16,
    pub ap: u16,
    pub power: u16,
    pub defense: u16,
    pub agility: u16,
    pub intellect: u16,
    pub steal: ItemRef,
    /// Index into the drop chance table (`EnemyDrop.rate`). The byte after it is battle-only flags, kept as read.
    pub steal_rate: u8,
    pub drop: ItemRef,
    /// Index into the drop chance table (`EnemyDrop.rate`). The byte after it is battle-only flags, kept as read.
    pub drop_rate: u8,
    pub action_blocks: [ActionBlock; 4],
    /// Resistance levels, `0` to `7`, for fire, frost, thunder, earth, wind, holy, psionic, status and death.
    pub resistances: [u8; 9],
}

/// One encounter block: the encounters of an area and the enemies they draw.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct EncounterBlock {
    pub encounters: Vec<Encounter>,
    pub enemies: Vec<Enemy>,
}

fn serialize_name<S: serde::Serializer>(
    name: &[u8; NAME_BYTES],
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    serializer.serialize_str(&name_text(name))
}

impl EncounterBlock {
    /// Reads an encounter block from an area's encounter payload.
    pub fn parse(payload: &[u8]) -> Result<Self> {
        if payload.len() < BLOCK_BYTES {
            return Err(Error::EncounterBlockShort { bytes: payload.len() });
        }
        let encounters = (0..ENCOUNTERS)
            .map(|index| {
                let record = &payload[index * ENCOUNTER_BYTES..(index + 1) * ENCOUNTER_BYTES];
                Encounter {
                    enemies: record[..ENCOUNTER_ENEMIES].try_into().expect("eight enemy indexes"),
                    appearance_rate: record[ENCOUNTER_ENEMIES],
                }
            })
            .collect();
        let enemies = (0..ENEMIES)
            .map(|index| {
                let start = ENEMY_BASE + index * ENEMY_BYTES;
                enemy(&payload[start..start + ENEMY_BYTES])
            })
            .collect();
        Ok(Self { encounters, enemies })
    }
}

fn action_block(record: &[u8], at: usize) -> ActionBlock {
    ActionBlock { condition: record[at], ai_params: bytes(record, at + 1), skills: bytes(record, at + 8) }
}

fn enemy(record: &[u8]) -> Enemy {
    Enemy {
        name: bytes(record, 0x00),
        enemy_id: u16_at(record, 0x08),
        choice_ai: record[0x0a],
        target_preference: record[0x0e],
        zenny: u16_at(record, 0x10),
        exp: u16_at(record, 0x12),
        level: record[0x14],
        initial_skills: bytes(record, 0x18),
        hp: u16_at(record, 0x20),
        ap: u16_at(record, 0x22),
        power: u16_at(record, 0x24),
        defense: u16_at(record, 0x26),
        agility: u16_at(record, 0x28),
        intellect: u16_at(record, 0x2a),
        steal: ItemRef { index: record[0x2c], item_type: record[0x2d] },
        steal_rate: record[0x2e],
        drop: ItemRef { index: record[0x30], item_type: record[0x31] },
        drop_rate: record[0x32],
        action_blocks: [0x34, 0x44, 0x54, 0x64].map(|at| action_block(record, at)),
        resistances: bytes(record, 0x78),
    }
}

/// Reads an enemy's name from its JSON text, which must fit in [`NAME_BYTES`] bytes.
fn deserialize_name<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<[u8; NAME_BYTES], D::Error> {
    let text = <String as serde::Deserialize>::deserialize(deserializer)?;
    let bytes = name_field(&text, NAME_BYTES).map_err(<D::Error as serde::de::Error>::custom)?;
    Ok(bytes.try_into().expect("an enemy name is NAME_BYTES long"))
}

/// Writes a name into an enemy record: its stored bytes, color codes included.
fn put_name(record: &mut [u8], name: &[u8; NAME_BYTES]) {
    record[..NAME_BYTES].copy_from_slice(name);
}

fn put_u16(record: &mut [u8], at: usize, value: u16) {
    record[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

/// Writes one encounter's enemy indexes and appearance rate into its 9-byte record.
pub fn put_encounter(record: &mut [u8], encounter: &Encounter) -> std::result::Result<(), String> {
    record[..ENCOUNTER_ENEMIES].copy_from_slice(&encounter.enemies);
    record[ENCOUNTER_ENEMIES] = encounter.appearance_rate;
    Ok(())
}

/// Writes one enemy's named bytes into its 0x88-byte record. Bytes no field names keep their stored values.
pub fn put_enemy(record: &mut [u8], enemy: &Enemy) -> std::result::Result<(), String> {
    put_name(record, &enemy.name);
    put_u16(record, 0x08, enemy.enemy_id);
    record[0x0a] = enemy.choice_ai;
    record[0x0e] = enemy.target_preference;
    put_u16(record, 0x10, enemy.zenny);
    put_u16(record, 0x12, enemy.exp);
    record[0x14] = enemy.level;
    record[0x18..0x20].copy_from_slice(&enemy.initial_skills);
    put_u16(record, 0x20, enemy.hp);
    put_u16(record, 0x22, enemy.ap);
    put_u16(record, 0x24, enemy.power);
    put_u16(record, 0x26, enemy.defense);
    put_u16(record, 0x28, enemy.agility);
    put_u16(record, 0x2a, enemy.intellect);
    record[0x2c] = enemy.steal.index;
    record[0x2d] = enemy.steal.item_type;
    record[0x2e] = enemy.steal_rate;
    record[0x30] = enemy.drop.index;
    record[0x31] = enemy.drop.item_type;
    record[0x32] = enemy.drop_rate;
    for (at, block) in [0x34, 0x44, 0x54, 0x64].into_iter().zip(&enemy.action_blocks) {
        record[at] = block.condition;
        record[at + 1..at + 8].copy_from_slice(&block.ai_params);
        record[at + 8..at + 16].copy_from_slice(&block.skills);
    }
    record[0x78..0x81].copy_from_slice(&enemy.resistances);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A block with one encounter (enemies 0 and 1, rate 2) and one enemy "Worker" with id 12.
    fn block() -> Vec<u8> {
        let mut block = vec![0xFF; BLOCK_BYTES];
        block[..9].copy_from_slice(&[0, 1, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 2]);
        let at = ENEMY_BASE;
        block[at..at + 8].copy_from_slice(b"Worker\0\0");
        block[at + 0x08..at + 0x0a].copy_from_slice(&12u16.to_le_bytes());
        block[at + 0x14] = 5;
        block[at + 0x78..at + 0x81].copy_from_slice(&[3, 1, 1, 1, 1, 1, 1, 0, 1]);
        block
    }

    #[test]
    fn a_encounter_block_reads_its_encounters_and_enemies() {
        let parsed = EncounterBlock::parse(&block()).unwrap();
        assert_eq!(parsed.encounters.len(), ENCOUNTERS);
        assert_eq!(
            parsed.encounters[0],
            Encounter { enemies: [0, 1, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF], appearance_rate: 2 }
        );
        assert_eq!(parsed.enemies.len(), ENEMIES);
        let worker = &parsed.enemies[0];
        assert_eq!(name_text(&worker.name), "Worker");
        assert_eq!((worker.enemy_id, worker.level), (12, 5));
        assert_eq!(worker.resistances, [3, 1, 1, 1, 1, 1, 1, 0, 1]);
    }

    #[test]
    fn a_short_block_is_an_error() {
        assert!(matches!(EncounterBlock::parse(&block()[..100]), Err(Error::EncounterBlockShort { bytes: 100 })));
    }
}
