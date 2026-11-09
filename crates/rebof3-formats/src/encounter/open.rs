//! The encounter block as open files: JSON out, and the JSON back over the stored block.

use super::{BLOCK_BYTES, ENCOUNTER_BYTES, ENCOUNTERS, ENEMIES, ENEMY_BASE, ENEMY_BYTES, EncounterBlock};
use crate::error::{Error, Result};

/// The JSON of one area's encounter block: its eight encounters and eight enemy records. The enemy
/// indexes of an encounter name the records in `enemies`.
pub fn export(payload: &[u8]) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec_pretty(&EncounterBlock::parse(payload)?).expect("an encounter block serializes"))
}

/// The encounter block of an area (`area-encounters`), from its JSON and its stored bytes. `base` is the stored
/// block: the bytes no field names keep their stored values.
pub fn import(json: &[u8], base: &[u8]) -> Result<Vec<u8>> {
    let block: EncounterBlock =
        serde_json::from_slice(json).map_err(|error| Error::Import(format!("encounter JSON: {error}")))?;
    if base.len() != BLOCK_BYTES || block.encounters.len() != ENCOUNTERS || block.enemies.len() != ENEMIES {
        return Err(Error::Import(format!(
            "an encounter block is {} bytes, {} encounters and {} enemies",
            BLOCK_BYTES, ENCOUNTERS, ENEMIES
        )));
    }
    let mut bytes = base.to_vec();
    for (index, value) in block.encounters.iter().enumerate() {
        let at = index * ENCOUNTER_BYTES;
        super::put_encounter(&mut bytes[at..at + ENCOUNTER_BYTES], value).map_err(Error::Import)?;
    }
    for (index, value) in block.enemies.iter().enumerate() {
        let at = ENEMY_BASE + index * ENEMY_BYTES;
        super::put_enemy(&mut bytes[at..at + ENEMY_BYTES], value).map_err(Error::Import)?;
    }
    Ok(bytes)
}
