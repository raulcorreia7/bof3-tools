//! What the executable and an area's program say about the area's field, beside its own entries: the
//! setup stream that places the actors ([`setup`]), the encounter zones and the facing steps.
//!
//! None of this is an entry format of its own. The tables sit in `SLUS_004.22` and the setup stream
//! in the area program (the code image loaded at [`PROGRAM_ADDRESS`]), so the readers take the
//! executable's bytes and the program's bytes. Every address names a decomp symbol.

pub mod setup;

use psx_formats::bytes::u32_le;
use psx_formats::exe;

use crate::error::{Error, Result};

/// Where an area program loads (`AreaLoad` records and setup streams point into it; `g_AreaLoads`
/// holds addresses from `0x801F2C18`).
pub const PROGRAM_ADDRESS: u32 = 0x801F_2C00;
/// `g_AreaLoads` (`exe/slus_004_22@0x8017F974`): one `AreaLoad` pointer per area; `func_8019FAA0`
/// passes the record's first word, the setup stream, to `Area_RunSetupStream`.
pub const AREA_LOADS: u32 = 0x8017_F974;
/// `g_EncounterZones` (`exe/slus_004_22@0x80181164`): one pointer per area into
/// `g_AreaEncounterZones`, read by `func_801B3C3C` (`emi/etc/game/00@0x801B3C3C`).
pub const ENCOUNTER_ZONES: u32 = 0x8018_1164;
/// `g_AreaEncounterZones` (`exe/slus_004_22@0x8017FC94`): the zone rows of every area, back to back.
pub const AREA_ENCOUNTER_ZONES: u32 = 0x8017_FC94;
/// `MAP_ZONE_COUNT`: the rows of `g_AreaEncounterZones` (`src/engine/slus/D_8017FC94.c`).
pub const ZONE_ROWS: usize = 666;
/// `g_FacingSteps` (`exe/slus_004_22@0x80181B94`): the cell step of each of the eight facings, as
/// 16.16 `x` and `y` words.
pub const FACING_STEPS: u32 = 0x8018_1B94;
/// Where `ETC/GAME.EMI`'s code image loads.
pub const GAME_CODE: u32 = 0x8019_5800;
/// `D_801C839C` (`emi/etc/game/00@0x801C839C`): per facing, the actor script `func_8015BAC4`
/// (`exe/slus_004_22@0x8015BAC4`) starts and, at the odd byte (`D_801C839D`), the flip it latches.
pub const FACING_SCRIPTS: u32 = 0x801C_839C;
/// Areas in `g_AreaLoads` and `g_EncounterZones`.
pub const AREAS: usize = 200;
/// Facings a step table row exists for.
pub const FACINGS: usize = 8;

/// Bytes in one zone row.
pub const ZONE_BYTES: usize = 8;

/// The executable's text, addressed by RAM address.
pub struct Executable<'a>(exe::Executable<'a>);

impl<'a> Executable<'a> {
    /// Reads a PS-X EXE header.
    pub fn new(bytes: &'a [u8]) -> Result<Self> {
        exe::Executable::parse(bytes).map(Self).map_err(|error| Error::Field(error.to_string()))
    }

    /// The `length` bytes at RAM `address`.
    pub fn bytes(&self, address: u32, length: usize) -> Result<&'a [u8]> {
        self.0.bytes_at(address, length).ok_or_else(|| Error::Field(format!("the executable does not hold {length} bytes at {address:#010X}")))
    }

    /// The word at RAM `address`.
    pub fn word(&self, address: u32) -> Result<u32> {
        Ok(u32_le(self.bytes(address, 4)?, 0))
    }

    /// The address of area `area`'s `AreaLoad` record (`g_AreaLoads[area]`).
    pub fn area_load(&self, area: usize) -> Result<u32> {
        check_area(area)?;
        self.word(AREA_LOADS + 4 * area as u32)
    }

    /// The eight facing steps, in 16.16 cells: `[x, y]` per facing.
    pub fn facing_steps(&self) -> Result<[[i32; 2]; FACINGS]> {
        let bytes = self.bytes(FACING_STEPS, FACINGS * 8)?;
        let mut steps = [[0; 2]; FACINGS];
        for (step, row) in steps.iter_mut().zip(bytes.as_chunks::<8>().0) {
            *step = [i32::from_le_bytes([row[0], row[1], row[2], row[3]]), i32::from_le_bytes([row[4], row[5], row[6], row[7]])];
        }
        Ok(steps)
    }

    /// The encounter zone rows of area `area`: the rows from its `g_EncounterZones` pointer up to the
    /// next area's list or the table's end. `func_801B3C3C` scans them in order and takes the first
    /// rectangle that holds the party's cell; no row ends a list, so a point no row holds runs on into
    /// the next list.
    pub fn zones(&self, area: usize) -> Result<Vec<Zone>> {
        check_area(area)?;
        let table_end = AREA_ENCOUNTER_ZONES + (ZONE_ROWS * ZONE_BYTES) as u32;
        let pointers: Vec<u32> = (0..AREAS as u32).map(|index| self.word(ENCOUNTER_ZONES + 4 * index)).collect::<Result<_>>()?;
        let start = pointers[area];
        if !(AREA_ENCOUNTER_ZONES..table_end).contains(&start) {
            return Err(Error::Field(format!("area {area}'s zone list {start:#010X} is outside g_AreaEncounterZones")));
        }
        let end = pointers.iter().copied().filter(|&pointer| pointer > start).min().unwrap_or(table_end).min(table_end);
        let rows = ((end - start) as usize) / ZONE_BYTES;
        let bytes = self.bytes(start, rows * ZONE_BYTES)?;
        Ok(bytes
            .as_chunks::<ZONE_BYTES>()
            .0
            .iter()
            .enumerate()
            .map(|(index, row)| Zone { address: start + (index * ZONE_BYTES) as u32, first: [row[0], row[1]], last: [row[2], row[3]], rate: row[4], zone_type: row[5], seed: row[6], pick: row[7] })
            .collect())
    }
}

/// The script and flip of each facing, `[script, flip]`, from the game code image (the payload that
/// loads at [`GAME_CODE`]).
pub fn facing_scripts(code: &[u8]) -> Result<[[u8; 2]; FACINGS]> {
    let at = (FACING_SCRIPTS - GAME_CODE) as usize;
    let bytes = code.get(at..at + 2 * FACINGS).ok_or_else(|| Error::Field("the game code image does not reach D_801C839C".into()))?;
    let mut scripts = [[0; 2]; FACINGS];
    for (script, pair) in scripts.iter_mut().zip(bytes.as_chunks::<2>().0) {
        *script = *pair;
    }
    Ok(scripts)
}

/// One `EncounterZone` row: a cell rectangle and what the field reads when the party enters it
/// (`func_801B3C3C`; the fields in `src/engine/slus/D_8017FC94.c`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Zone {
    /// The row's RAM address in the executable.
    pub address: u32,
    /// The first cell, `[x, y]`, inclusive.
    pub first: [u8; 2],
    /// The last cell, `[x, y]`, inclusive.
    pub last: [u8; 2],
    /// The step-threshold row `Area_RollEncounterThreshold` (`emi/etc/game/00@0x801B3CCC`) reads.
    pub rate: u8,
    /// The zone type, stored to `D_80143F03` (the setup stream's test 8 reads it).
    pub zone_type: u8,
    /// The music selection seed and picker flag (`func_801A0380`).
    pub seed: u8,
    pub pick: u8,
}

impl Zone {
    /// Whether the zone holds cell `x`, `y`.
    pub fn holds(&self, x: u8, y: u8) -> bool {
        (self.first[0]..=self.last[0]).contains(&x) && (self.first[1]..=self.last[1]).contains(&y)
    }
}

fn check_area(area: usize) -> Result<()> {
    (area < AREAS).then_some(()).ok_or_else(|| Error::Field(format!("area {area} is past the {AREAS} areas")))
}
