//! What the executable and an area's program say about the area's field, beside its own entries: the
//! setup stream that places the actors ([`setup`]), the encounter zones and the facing steps.
//!
//! None of this is an entry format of its own. The tables sit in `SLUS_004.22` and the setup stream
//! in the area program (the code image loaded at [`PROGRAM_ADDRESS`]), so the readers take the
//! executable's bytes and the program's bytes. Every address names a decomp symbol.

pub mod setup;

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
/// Areas in `g_AreaLoads` and `g_EncounterZones`.
pub const AREAS: usize = 200;
/// Facings a step table row exists for.
pub const FACINGS: usize = 8;

/// The magic of a PlayStation executable, and where its header keeps the text address; the text
/// starts at file offset `0x800` (psx-spx, "CDROM File Formats", "PS-X EXE").
const EXE_MAGIC: &[u8; 8] = b"PS-X EXE";
const EXE_TEXT_ADDRESS: usize = 0x18;
const EXE_TEXT_START: usize = 0x800;
/// Bytes in one zone row.
pub const ZONE_BYTES: usize = 8;

/// The executable's text, addressed by RAM address.
pub struct Executable<'a> {
    bytes: &'a [u8],
    text: u32,
}

impl<'a> Executable<'a> {
    /// Reads a PS-X EXE header.
    pub fn new(bytes: &'a [u8]) -> Result<Self> {
        if bytes.get(..EXE_MAGIC.len()) != Some(&EXE_MAGIC[..]) {
            return Err(Error::Field("the executable is not a PS-X EXE".into()));
        }
        let text =
            word_at(bytes, EXE_TEXT_ADDRESS).ok_or_else(|| Error::Field("the executable's header is short".into()))?;
        Ok(Self { bytes, text })
    }

    /// The `length` bytes at RAM `address`.
    pub fn bytes(&self, address: u32, length: usize) -> Result<&'a [u8]> {
        let offset = address.checked_sub(self.text).map(|offset| EXE_TEXT_START + offset as usize);
        offset
            .and_then(|offset| self.bytes.get(offset..offset + length))
            .ok_or_else(|| Error::Field(format!("the executable does not hold {length} bytes at {address:#010X}")))
    }

    /// The word at RAM `address`.
    pub fn word(&self, address: u32) -> Result<u32> {
        let bytes = self.bytes(address, 4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
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
            *step = [
                i32::from_le_bytes([row[0], row[1], row[2], row[3]]),
                i32::from_le_bytes([row[4], row[5], row[6], row[7]]),
            ];
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
        let pointers: Vec<u32> =
            (0..AREAS as u32).map(|index| self.word(ENCOUNTER_ZONES + 4 * index)).collect::<Result<_>>()?;
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
            .map(|(index, row)| Zone {
                address: start + (index * ZONE_BYTES) as u32,
                first: [row[0], row[1]],
                last: [row[2], row[3]],
                rate: row[4],
                kind: row[5],
                seed: row[6],
                pick: row[7],
            })
            .collect())
    }
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
    /// The zone kind, stored to `D_80143F03` (the setup stream's test 8 reads it).
    pub kind: u8,
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

fn word_at(bytes: &[u8], at: usize) -> Option<u32> {
    bytes.get(at..at + 4).map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An executable whose text starts at `0x8017F000` and holds the tables the readers need.
    fn exe() -> Vec<u8> {
        let text = 0x8017_F000u32;
        let mut bytes = vec![0u8; EXE_TEXT_START + 0x3000];
        bytes[..8].copy_from_slice(EXE_MAGIC);
        bytes[EXE_TEXT_ADDRESS..EXE_TEXT_ADDRESS + 4].copy_from_slice(&text.to_le_bytes());
        let mut put = |address: u32, data: &[u8]| {
            let at = EXE_TEXT_START + (address - text) as usize;
            bytes[at..at + data.len()].copy_from_slice(data);
        };
        // Area 0's list has two rows, area 1's one; the rest point at area 1's.
        for area in 0..AREAS as u32 {
            let start = AREA_ENCOUNTER_ZONES + if area == 0 { 0 } else { 16 };
            put(ENCOUNTER_ZONES + 4 * area, &start.to_le_bytes());
        }
        put(AREA_ENCOUNTER_ZONES, &[1, 2, 10, 12, 2, 0, 1, 1, 0, 0, 255, 255, 0, 1, 1, 1, 5, 5, 6, 6, 3, 2, 9, 0]);
        put(AREA_LOADS + 4, &0x801F_2C18u32.to_le_bytes());
        put(FACING_STEPS, &(-32768i32).to_le_bytes());
        bytes
    }

    #[test]
    fn the_tables_read_by_address() {
        let bytes = exe();
        let exe = Executable::new(&bytes).expect("an executable");
        assert_eq!(exe.area_load(1).unwrap(), 0x801F_2C18);
        assert_eq!(exe.facing_steps().unwrap()[0], [-32768, 0]);
        let zones = exe.zones(0).unwrap();
        assert_eq!(zones.len(), 2);
        assert_eq!((zones[0].first, zones[0].last, zones[0].rate), ([1, 2], [10, 12], 2));
        assert!(zones[0].holds(10, 12) && !zones[0].holds(11, 12));
        assert_eq!(exe.zones(1).unwrap().len(), ZONE_ROWS - 2);
        assert!(exe.zones(AREAS).is_err());
        assert!(Executable::new(b"not an exe").is_err());
    }
}
