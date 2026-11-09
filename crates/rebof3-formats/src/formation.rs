//! A random-encounter formation as the field places it for battle.
//!
//! An area's encounter block (`0x800E4000`, [`crate::encounter`]) lists eight encounters of eight enemy
//! indexes. When one is rolled, `Area_BuildEncounter` (`emi/etc/game/00@0x801C60E0`) runs:
//!
//! - `Area_RollEncounterEnemies` (`emi/etc/game/00@0x801C6AEC`) keeps each listed slot with the chance
//!   `s_EntitySpawnChances[slot]`, a run-time roll. [`place`] keeps every listed slot.
//! - `Area_PlaceEncounterEnemies` (`emi/etc/game/00@0x801C6BB4`) places the `n`-th kept enemy of `N` at
//!   `s_EncounterEnemyOffsets[n + N (N - 1) / 2]` plus five cells in x, turned a quarter `(4 - d) % 4`
//!   times for the scroll direction `d`, from the formation origin (the party's cell). It then tries
//!   `s_EntitySpawnOffsets` in order until the map accepts the cell (`func_801C7518`,
//!   `func_801C6EE0`); the first offset is `(0, 0)`, so [`place`] gives the place the map accepts first.
//! - `func_800ABC30` (`emi/battle/battle/15@0x800ABC30`) hands each kept enemy to `Battle_InitEnemy`
//!   (`emi/battle/battle/15@0x800ABCE0`), which looks the enemy record's `+0x16` word up with
//!   `Area_FindSceneRecord` (`exe/slus_004_22@0x8014DD3C`): the scene record gives the sprite's page, row,
//!   CLUT selector, palette kind and script section ([`crate::scene`]), with work `+0x24` cleared.
//!
//! The two tables are read from `ETC/GAME.EMI` entry 0, the code image that holds them.

use rebof3_emi::record::u16_at;
use rebof3_emi::text::codec::name_text;

use crate::animation::Animation;
use crate::encounter::{self, ENCOUNTER_ENEMIES, ENEMY_BASE, ENEMY_BYTES, NAME_BYTES};
use crate::error::{Error, Result};
use crate::scene::{SceneRecord, SceneTable};
use crate::script::program::Program;
use crate::sprite::Placement;

/// The address of `s_EncounterEnemyOffsets` (`emi/etc/game/00`): one 16.16 `x`, `y` pair per kept enemy of
/// every formation size, 1 to 8.
pub const ENEMY_OFFSETS: u32 = 0x801C_D620;
/// Pairs in `s_EncounterEnemyOffsets`: `1 + 2 + … + 8`.
pub const ENEMY_OFFSET_COUNT: usize = 36;
/// Bytes of one pair (`Point2DL`).
const PAIR_BYTES: usize = 8;
/// `TO_COORD(5)`: the cells every enemy offset adds in x.
pub const FRONT_CELLS: i32 = 5 << 16;
/// The enemy record word `Battle_InitEnemy` looks the scene record up with (`D_800E405E`).
pub const SCENE_ID_AT: usize = 0x16;
/// Scroll directions `Area_BuildEncounter` turns a formation by.
pub const DIRECTIONS: u8 = 4;
/// The `+0x24` flags `Battle_InitEnemy` leaves an enemy with.
pub const ENEMY_FLAGS: u8 = 0;

/// `s_EncounterEnemyOffsets`, read from the code image that holds it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Offsets {
    pairs: Vec<(i32, i32)>,
}

impl Offsets {
    /// The table from `code`, the payload of the entry loaded at `load`.
    pub fn from_code(code: &[u8], load: u32) -> Result<Self> {
        let start = ENEMY_OFFSETS
            .checked_sub(load)
            .map(|offset| offset as usize)
            .filter(|start| start + ENEMY_OFFSET_COUNT * PAIR_BYTES <= code.len())
            .ok_or_else(|| {
                Error::Formation(format!("the code image at {load:#010X} does not hold s_EncounterEnemyOffsets"))
            })?;
        let word = |at: usize| i32::from_le_bytes([code[at], code[at + 1], code[at + 2], code[at + 3]]);
        let pairs = (0..ENEMY_OFFSET_COUNT)
            .map(|index| start + index * PAIR_BYTES)
            .map(|at| (word(at), word(at + 4)))
            .collect();
        Ok(Self { pairs })
    }

    /// The table from its pairs.
    pub fn from_pairs(pairs: Vec<(i32, i32)>) -> Self {
        Self { pairs }
    }
}

/// One enemy of a formation, where the field places it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Placed {
    /// The encounter slot, 0 to 7.
    pub slot: usize,
    /// The enemy record index the slot names.
    pub enemy: usize,
    /// The enemy's name.
    pub name: String,
    /// The enemy record's `+0x16` word, the scene record `Battle_InitEnemy` looks up.
    pub scene_id: u16,
    /// The scene record with that id, when the area's table has one.
    pub scene: Option<SceneRecord>,
    /// The place, in 16.16 cells from the formation origin.
    pub x: i32,
    pub y: i32,
}

/// The enemies of encounter `encounter` of `block`, every listed slot kept, placed for scroll direction
/// `direction` (0 to 3), with their scene records from `scenes`.
pub fn place(
    block: &[u8],
    scenes: Option<&SceneTable>,
    encounter: usize,
    offsets: &Offsets,
    direction: u8,
) -> Result<Vec<Placed>> {
    let parsed = encounter::EncounterBlock::parse(block)?;
    let listed = parsed
        .encounters
        .get(encounter)
        .ok_or_else(|| Error::Formation(format!("the block has no encounter {encounter}")))?;
    let kept: Vec<(usize, usize)> = listed
        .enemies
        .iter()
        .take(ENCOUNTER_ENEMIES)
        .enumerate()
        .filter(|(_, enemy)| **enemy != encounter::EMPTY_SLOT)
        .map(|(slot, enemy)| (slot, usize::from(*enemy)))
        .collect();
    let count = kept.len();
    let first = count * count.saturating_sub(1) / 2;

    let mut placed = Vec::with_capacity(count);
    for (order, (slot, enemy)) in kept.into_iter().enumerate() {
        let &(dx, dy) = offsets
            .pairs
            .get(first + order)
            .ok_or_else(|| Error::Formation(format!("s_EncounterEnemyOffsets has no pair {}", first + order)))?;
        let (x, y) = turn((dx + FRONT_CELLS, dy), direction);
        let record = ENEMY_BASE + enemy * ENEMY_BYTES;
        let fields = block
            .get(record..record + ENEMY_BYTES)
            .ok_or_else(|| Error::Formation(format!("slot {slot} names enemy {enemy}, past the block")))?;
        let scene_id = u16_at(fields, SCENE_ID_AT);
        let scene = scenes.and_then(|table| table.records().iter().find(|record| record.id == scene_id).copied());
        placed.push(Placed { slot, enemy, name: name_text(&fields[..NAME_BYTES]), scene_id, scene, x, y });
    }
    Ok(placed)
}

/// The VRAM placement `Area_FindSceneRecord` gives an enemy of scene record `scene`: page, row, the
/// selector's low byte (work `+0x27` is a byte) and the palette kind, with `Battle_InitEnemy`'s flags.
pub fn placement(scene: &SceneRecord) -> Placement {
    Placement::scene(scene.texture_page, scene.texture_v, scene.clut_select as u8, scene.texture_mode, ENEMY_FLAGS)
}

/// The animation stream table entry `entry` of the scene record's script section names, as
/// `Actor_BeginScript` (`exe/slus_004_22@0x8014D6DC`) reads it from the area script bank.
pub fn stream<'a>(program: &'a Program, scene: &SceneRecord, entry: usize) -> Option<&'a Animation> {
    let section = program.sections.get(usize::from(scene.script_section))?;
    let record = usize::from(*section.slots.get(entry)?);
    Some(&section.records.get(record)?.stream()?.animation)
}

/// `Area_PlaceEncounterEnemies`' turn: `(x, y)` becomes `(y, -x)` while the direction counts up to 4.
pub fn turn((mut x, mut y): (i32, i32), direction: u8) -> (i32, i32) {
    let mut step = direction % DIRECTIONS;
    while step != 0 {
        (x, y) = (y, -x);
        step = (step + 1) % DIRECTIONS;
    }
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A block whose encounter 0 lists enemies 1 and 3 in slots 0 and 2.
    fn block() -> Vec<u8> {
        let mut block = vec![0u8; encounter::BLOCK_BYTES];
        block[..encounter::ENCOUNTERS * encounter::ENCOUNTER_BYTES].fill(encounter::EMPTY_SLOT);
        block[0] = 1;
        block[2] = 3;
        for (enemy, id) in [(1usize, 0x0042u16), (3, 0x0107)] {
            let at = ENEMY_BASE + enemy * ENEMY_BYTES;
            block[at] = 0x21;
            block[at + SCENE_ID_AT..at + SCENE_ID_AT + 2].copy_from_slice(&id.to_le_bytes());
        }
        block
    }

    fn offsets() -> Offsets {
        Offsets::from_pairs((0..ENEMY_OFFSET_COUNT as i32).map(|index| (index << 16, -(index << 16))).collect())
    }

    #[test]
    fn a_formation_of_two_takes_the_pairs_after_the_formation_of_one() {
        let placed = place(&block(), None, 0, &offsets(), 0).expect("places");
        assert_eq!(placed.iter().map(|enemy| (enemy.slot, enemy.enemy)).collect::<Vec<_>>(), [(0, 1), (2, 3)]);
        assert_eq!((placed[0].x, placed[0].y), ((1 << 16) + FRONT_CELLS, -(1 << 16)), "pair 1 + 0");
        assert_eq!((placed[1].x, placed[1].y), ((2 << 16) + FRONT_CELLS, -(2 << 16)), "pair 1 + 1");
        assert_eq!(placed[1].scene_id, 0x0107);
        assert!(placed[0].scene.is_none());
    }

    #[test]
    fn the_turn_follows_the_loop_of_the_placement() {
        assert_eq!(turn((1, 2), 0), (1, 2));
        assert_eq!(turn((1, 2), 3), (2, -1), "one quarter");
        assert_eq!(turn((1, 2), 2), (-1, -2), "two quarters");
        assert_eq!(turn((1, 2), 1), (-2, 1), "three quarters");
    }

    #[test]
    fn the_table_reads_from_the_code_image() {
        let load = ENEMY_OFFSETS - 8;
        let mut code = vec![0u8; 8 + ENEMY_OFFSET_COUNT * PAIR_BYTES];
        code[8..12].copy_from_slice(&(3i32 << 16).to_le_bytes());
        let table = Offsets::from_code(&code, load).expect("reads");
        assert_eq!(table.pairs[0], (3 << 16, 0));
        assert!(Offsets::from_code(&code[..20], load).is_err());
    }
}
