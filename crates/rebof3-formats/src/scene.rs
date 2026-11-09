//! Area scene table codec (`0x800E3800`).
//!
//! One WORLD* archive's scene/step table: a `u32` count then 8-byte records.
//! The engine walks the records at `D_801459F4 = 0x800E3800 + 4` (`@0x8014DD3C`
//! matches the `id` and fills the work fields, returning 1 on a miss) and reads
//! the count at `0x800E3800` itself as one byte.
//!
//! The byte at `+0x7` is the record's **section index in the archive's area
//! script bank**: the reader stores it into `work+0x2C`, which the script-bank
//! reader `@0x8014D6DC` uses as `u32[bank + 4*lbu(work+0x2C)]`, and in all 200
//! payloads the records' values cover exactly `0..u32[script_bank]/4 - 1`. An
//! earlier reading of it as "the record ordinal" is superseded.
//!
//! Layout and reader evidence: `wiki: formats/area#0x800e3800-scenestep-table`.

use psx_formats::bytes::{push_u16_le, push_u32_le, u16_le, u32_le};
use serde::Serialize;

use rebof3_emi::Entry;

use crate::error::{Error, Result};
use crate::identity::difference;
use crate::registry::{Description, EMI_EXTENSION, Format, RoundTrip, Selector, State};

/// The load address of the table.
const LOAD_ADDRESS: u32 = 0x800e_3800;

/// The table as a format-registry row.
pub static ACTOR_DEFS: Description = Description {
    format: Format::AreaActorDefsTable,
    name: "area actor defs table (0x800E3800)",
    state: State::Codec,
    spec_state: "documented",
    codec_state: "decode+encode",
    evidence: "wiki: formats/area (`0x800E3800`)",
    extension: EMI_EXTENSION,
    selector: Selector::Load(&[LOAD_ADDRESS]),
    round_trip: RoundTrip::Payload(round_trip),
};

/// Decodes the table and encodes it again; `Err` names the first difference.
fn round_trip(_entry: &Entry, payload: &[u8]) -> Result<(), String> {
    let table = SceneTable::decode(payload).map_err(|error| error.to_string())?;
    difference(&table.encode(), payload)
}

/// One scene record, in bytes.
pub const RECORD_BYTES: usize = 8;
/// The count that precedes the records.
pub const COUNT_BYTES: usize = 4;
/// The largest count the engine's byte-wide read can carry (`@0x8014DD3C`
/// loads the count with `lbu`).
pub const MAX_RECORDS: u32 = 0xff;

/// One scene record, with the work field its byte reaches in parentheses.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct SceneRecord {
    /// Match key (`@0x8014DD3C`, `@0x80167E00`) and the id getter
    /// `emi/etc/game/00@0x801ACE9C` reads.
    pub id: u16,
    /// The record's second `u16`; its low byte reaches `work+0x27` (C `clut_select`).
    pub clut_select: u16,
    /// Reaches `work+0x25`; 7, 9, 11 and 13 in all 200 payloads (C `texture_page`).
    pub texture_page: u8,
    /// Reaches `work+0x26` (C `texture_v`).
    pub texture_v: u8,
    /// Reaches `work+0x28`; the 9 and 10 cases belong to actor `+0x06` (C `texture_mode`).
    pub texture_mode: u8,
    /// Section of the archive's area script bank this scene runs, `0`-based (C `script_section`).
    pub script_section: u8,
}

impl SceneRecord {
    fn read(record: &[u8]) -> Self {
        Self {
            id: u16_le(record, 0),
            clut_select: u16_le(record, 2),
            texture_page: record[4],
            texture_v: record[5],
            texture_mode: record[6],
            script_section: record[7],
        }
    }

    fn write(self, out: &mut Vec<u8>) {
        push_u16_le(out, self.id);
        push_u16_le(out, self.clut_select);
        out.push(self.texture_page);
        out.push(self.texture_v);
        out.push(self.texture_mode);
        out.push(self.script_section);
    }
}

/// One archive's scene records, in table order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SceneTable {
    records: Vec<SceneRecord>,
}

impl SceneTable {
    /// Reads a `0x800E3800` payload.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        let count = if bytes.len() >= COUNT_BYTES { u32_le(bytes, 0) } else { 0 };
        if count > MAX_RECORDS || bytes.len() != COUNT_BYTES + RECORD_BYTES * count as usize {
            return Err(Error::SceneTableShape { bytes: bytes.len(), count });
        }
        let records = (0..count as usize)
            .map(|index| {
                let at = COUNT_BYTES + index * RECORD_BYTES;
                SceneRecord::read(&bytes[at..at + RECORD_BYTES])
            })
            .collect();
        Ok(Self { records })
    }

    /// The records, in table order.
    pub fn records(&self) -> &[SceneRecord] {
        &self.records
    }

    /// The record at `index`, if the table holds one.
    pub fn record(&self, index: usize) -> Option<&SceneRecord> {
        self.records.get(index)
    }

    /// The sections the records name, in table order.
    pub fn sections(&self) -> Vec<u8> {
        self.records.iter().map(|record| record.script_section).collect()
    }

    /// The records as JSON, one object per scene in table order.
    pub fn json(&self) -> String {
        serde_json::to_string_pretty(&self.records).expect("scene records are serializable")
    }

    /// The payload bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(COUNT_BYTES + self.records.len() * RECORD_BYTES);
        push_u32_le(&mut out, self.records.len() as u32);
        for record in &self.records {
            record.write(&mut out);
        }
        out
    }
}
