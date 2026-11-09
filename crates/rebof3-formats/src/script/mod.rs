//! Script bank codec: the area (`0x800D3800`), battle (`0x800F0800`) and magic-effect
//! (`0x800C3800`) banks.
//!
//! One archive's script bank: a self-sizing directory, then its sections, each a `u16`
//! record-offset table and the record bytes. The reader is `Actor_BeginScript`
//! (`exe/slus_004_22@0x8014D6DC`, `decomp/src/engine/slus/Actor_BeginScript.c`):
//!
//! ```c
//! table = (u8*)g_ScriptBank;
//! table += ((s32*)table)[g_Task->unk_2C];                // section base
//! g_Task->script = table + ((u16*)table)[g_Task->unk_4B]; // record
//! ```
//!
//! so the directory holds L words: word 0 is `4*L`, the directory's size, which is also section 0's
//! base, and words 1..L-1 are strictly increasing section bases. `g_ScriptBank` points at
//! `0x800D3800`, the battle handlers swap in `0x800F0800` (`func_801E6C84`,
//! `emi/battle/battle/03@0x801E6C84`), and the magic-effect overlays swap in `0x800C3800`
//! (`BMagic_Effect.c` in `decomp/src/overlays/bmagic/`), all the same format. The 37 magic-effect
//! banks hold one section, so their directory is the single word `4`.
//!
//! Each section's first word is `2*M`, the size of its M offset words, and it doubles as the first
//! offset word. No record size is stored, so the table offsets are the only record boundaries. In
//! every retail payload the distinct offsets of each section's table are strictly increasing and the
//! spans between them tile from the table end to the section end exactly; a table entry may repeat
//! an earlier offset, in which case the record is shared with the earlier entry.
//!
//! `Actor_BeginScript` hands the record to `Actor_SeekScript` (`exe/slus_004_22@0x8014D910`), so a
//! record is an animation stream: [`program`] reads each record's steps as ops, and [`text`] writes
//! and reads them one op per line.
//!
//! Layout and reader evidence: `wiki: formats/area#0x800d3800-area-script-bank`.

pub mod program;
pub mod text;

use core::fmt::Write as _;

use psx_formats::bytes::{push_u16_le, push_u32_le, u16_le, u32_le};
use serde::Serialize;

use rebof3_emi::Entry;

use crate::error::{Error, Result};
use crate::identity::difference;
use crate::registry::{Derived, Description, EMI_EXTENSION, Format, RoundTrip, Selector, State};

/// The load address of an area's script bank.
const AREA_ADDRESS: u32 = 0x800d_3800;
/// The load address of the battle script bank.
const BATTLE_ADDRESS: u32 = 0x800f_0800;
/// The load address of the magic-effect script bank of the `BMAGIC*` archives.
const MAGIC_ADDRESS: u32 = 0x800c_3800;

/// The readers every script bank row cites.
const STREAM_EVIDENCE: &str = "Actor_BeginScript (exe/slus_004_22@0x8014D6DC) reads a record of g_ScriptBank; \
Actor_SeekScript (exe/slus_004_22@0x8014D910) and Actor_StageAnimStep, Actor_StepAnimLoop, Actor_StepAnimOnce, Actor_StepAnimReverse \
play it as an animation stream; wiki: formats/area (`0x800D3800`)";

/// The area script bank as a format-registry row.
pub static AREA: Description = Description {
    format: Format::AreaScriptBank,
    name: "area script bank (0x800D3800)",
    state: State::Codec,
    spec_state: "documented",
    codec_state: "decode+encode; ops and text",
    evidence: STREAM_EVIDENCE,
    extension: EMI_EXTENSION,
    selector: Selector::Load(&[AREA_ADDRESS]),
    round_trip: RoundTrip::Payload(round_trip),
    derive: Some(derive),
    open: None,
    kind: "area-script",
};

/// The battle script bank as a format-registry row.
pub static BATTLE: Description = Description {
    format: Format::BattleScriptBank,
    name: "battle script bank (0x800F0800)",
    state: State::Codec,
    spec_state: "documented",
    codec_state: "decode+encode; ops and text",
    evidence: STREAM_EVIDENCE,
    extension: EMI_EXTENSION,
    selector: Selector::Load(&[BATTLE_ADDRESS]),
    round_trip: RoundTrip::Payload(round_trip),
    derive: Some(derive),
    open: None,
    kind: "battle-script",
};

/// The magic-effect script bank as a format-registry row.
pub static MAGIC: Description = Description {
    format: Format::MagicTable,
    name: "magic-effect script bank (0x800C3800)",
    state: State::Codec,
    spec_state: "documented",
    codec_state: "decode+encode; ops and text",
    evidence: STREAM_EVIDENCE,
    extension: EMI_EXTENSION,
    selector: Selector::Load(&[MAGIC_ADDRESS]),
    round_trip: RoundTrip::Payload(round_trip),
    derive: Some(derive),
    open: None,
    kind: "bmagic-table",
};

/// Decodes the bank to its ops and encodes it again; `Err` names the first difference.
fn round_trip(_entry: &Entry, payload: &[u8]) -> Result<(), String> {
    let program = program::Program::decode(payload).map_err(|error| error.to_string())?;
    difference(&program.encode().map_err(|error| error.to_string())?, payload)
}

/// The size of one directory word: a section base.
const DIRECTORY_WORD: usize = 4;
/// The size of one table word: a record offset.
const TABLE_WORD: usize = 2;
/// The smallest bank: a one-word directory and one section's table word.
const SMALLEST_BANK: usize = DIRECTORY_WORD + TABLE_WORD;

/// One section: its stored offset words and the record bytes they name.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ScriptSection {
    /// The size of the offset words, in bytes: `2 * M`, M the entry count.
    /// It is the first stored offset word as well.
    pub table_bytes: u16,
    /// The M stored offset words, relative to the section base, in table
    /// order. `offsets[0]` is the stored size word.
    pub offsets: Vec<u16>,
    /// For each table entry, the index into `records` of the record it
    /// names.
    pub slots: Vec<u16>,
    /// The record bytes, one per distinct offset, in first-appearance
    /// order. Record i runs from offset i to offset i + 1; the last runs to
    /// the section end.
    pub records: Vec<Vec<u8>>,
}

/// One archive's script bank: the directory and its sections.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ScriptBank {
    /// The directory's own size, in bytes: `4 * L`, L the section count.
    pub dir_bytes: u32,
    /// The sections, in directory order; the last runs to the end of the
    /// payload.
    pub sections: Vec<ScriptSection>,
}

impl ScriptBank {
    /// Reads a `0x800D3800`, `0x800F0800` or `0x800C3800` payload.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        let len = bytes.len();
        if len < SMALLEST_BANK {
            return Err(Error::ScriptBankShape("the payload is shorter than a directory word and a table word"));
        }

        let dir_bytes = u32_le(bytes, 0) as usize;
        if dir_bytes < DIRECTORY_WORD || !dir_bytes.is_multiple_of(DIRECTORY_WORD) || dir_bytes > len {
            return Err(Error::ScriptBankShape("the directory size is not 4*L inside the payload"));
        }

        let section_count = dir_bytes / DIRECTORY_WORD;
        let mut bases = vec![dir_bytes];
        for index in 1..section_count {
            let base = u32_le(bytes, DIRECTORY_WORD * index) as usize;
            if base <= bases[index - 1] || base >= len {
                return Err(Error::ScriptBankShape("a section offset is not strictly increasing inside the payload"));
            }
            bases.push(base);
        }

        let mut ends = bases[1..].to_vec();
        ends.push(len);
        let mut sections = Vec::with_capacity(section_count);
        for (base, end) in bases.iter().zip(&ends) {
            sections.push(section(bytes, *base, *end)?);
        }
        Ok(Self { dir_bytes: dir_bytes as u32, sections })
    }

    /// The sections, in directory order.
    pub fn sections(&self) -> &[ScriptSection] {
        &self.sections
    }

    /// The section at `index`, if the bank holds one.
    pub fn section(&self, index: usize) -> Option<&ScriptSection> {
        self.sections.get(index)
    }

    /// The bank as JSON: the directory size and, per section, the stored
    /// table words, the record bytes as hex, and the record each table
    /// entry names.
    pub fn json(&self) -> String {
        #[derive(Serialize)]
        struct Section {
            table_bytes: u16,
            offsets: Vec<u16>,
            slots: Vec<u16>,
            records: Vec<String>,
        }
        #[derive(Serialize)]
        struct Document {
            dir_bytes: u32,
            sections: Vec<Section>,
        }
        let document = Document {
            dir_bytes: self.dir_bytes,
            sections: self
                .sections
                .iter()
                .map(|section| Section {
                    table_bytes: section.table_bytes,
                    offsets: section.offsets.clone(),
                    slots: section.slots.clone(),
                    records: section.records.iter().map(|record| hex(record)).collect(),
                })
                .collect(),
        };
        serde_json::to_string_pretty(&document).expect("the script bank document is serializable")
    }

    /// The payload bytes.
    pub fn encode(&self) -> Vec<u8> {
        let dir_bytes = DIRECTORY_WORD * self.sections.len();
        let mut bases = vec![dir_bytes];
        for section in &self.sections {
            bases.push(bases[bases.len() - 1] + section_bytes(section));
        }
        let mut out = Vec::with_capacity(*bases.last().expect("the bank holds at least one section"));
        push_u32_le(&mut out, dir_bytes as u32);
        for base in &bases[1..bases.len() - 1] {
            push_u32_le(&mut out, *base as u32);
        }
        for section in &self.sections {
            write_section(&mut out, section);
        }
        out
    }
}

/// A decoded section's payload bytes: its offset words, then its records.
fn section_bytes(section: &ScriptSection) -> usize {
    section.table_bytes as usize + section.records.iter().map(Vec::len).sum::<usize>()
}

fn write_section(out: &mut Vec<u8>, section: &ScriptSection) {
    push_u16_le(out, section.table_bytes);
    for offset in section.offsets.iter().skip(1) {
        push_u16_le(out, *offset);
    }
    for record in &section.records {
        out.extend_from_slice(record);
    }
}

/// One section's table words and record bytes, from `[base, end)` of `bytes`.
fn section(bytes: &[u8], base: usize, end: usize) -> Result<ScriptSection, Error> {
    if end - base < TABLE_WORD {
        return Err(Error::ScriptBankShape("a section is shorter than its table word"));
    }
    let table_bytes = u16_le(bytes, base) as usize;
    if table_bytes == 0 || !table_bytes.is_multiple_of(TABLE_WORD) || base + table_bytes > end {
        return Err(Error::ScriptBankShape("a section's table size is not 2*M inside the section"));
    }
    let count = table_bytes / TABLE_WORD;
    let offsets: Vec<u16> = (0..count).map(|index| u16_le(bytes, base + TABLE_WORD * index)).collect();
    let limit = end - base;
    if offsets.iter().any(|offset| *offset as usize > limit) {
        return Err(Error::ScriptBankShape("a record offset runs past the section end"));
    }
    let mut distinct: Vec<usize> = Vec::with_capacity(count);
    for offset in &offsets {
        if !distinct.contains(&(*offset as usize)) {
            distinct.push(*offset as usize);
        }
    }
    if distinct.windows(2).any(|pair| pair[1] <= pair[0]) {
        return Err(Error::ScriptBankShape("record offsets repeat out of order"));
    }
    let mut bounds = distinct[1..].to_vec();
    bounds.push(limit);
    let mut records = Vec::with_capacity(distinct.len());
    for (start, stop) in distinct.iter().zip(&bounds) {
        records.push(bytes[base + *start..base + *stop].to_vec());
    }
    let slots: Vec<u16> = offsets
        .iter()
        .map(|offset| distinct.iter().position(|candidate| *candidate == *offset as usize).unwrap() as u16)
        .collect();
    Ok(ScriptSection { table_bytes: table_bytes as u16, offsets, slots, records })
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// The script records as JSON and the ops as [`text`]. The reader is `Actor_BeginScript` in
/// `decomp/src/engine/slus/Actor_BeginScript.c`; the layout is
/// `wiki: formats/area#0x800d3800-area-script-bank`.
fn derive(_load_arg: u32, payload: &[u8]) -> Result<Vec<Derived>> {
    let bank = ScriptBank::decode(payload)?;
    let listing = text::write(&program::Program::from_bank(&bank)).into_bytes();
    Ok(vec![
        Derived { role: "json", extension: "json", data: bank.json().into_bytes() },
        Derived { role: "script", extension: "txt", data: listing },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bank the round-trip test reads: three sections, the first with a
    /// repeated offset word.
    ///
    /// ```text
    /// dir  @0:   12, 33, 40
    /// sec0 @12:  words [8, 10, 10, 14], records [8,10) [10,14) [14,21)
    /// sec1 @33:  word [2], record [2,7)
    /// sec2 @40:  words [4, 8], records [4,8) [8,14)
    /// ```
    fn bank() -> Vec<u8> {
        let mut out = Vec::new();
        for word in [12u32, 33, 40] {
            push_u32_le(&mut out, word);
        }
        push_u16_le(&mut out, 8);
        for offset in [10u16, 10, 14] {
            push_u16_le(&mut out, offset);
        }
        out.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13]);
        push_u16_le(&mut out, 2);
        out.extend_from_slice(&[14, 15, 16, 17, 18]);
        push_u16_le(&mut out, 4);
        push_u16_le(&mut out, 8);
        out.extend_from_slice(&[19, 20, 21, 22, 23, 24, 25, 26, 27, 28]);
        out
    }

    #[test]
    fn a_bank_reads_its_sections_and_shares_a_record() {
        let payload = bank();
        let decoded = ScriptBank::decode(&payload).expect("a bank parses");
        assert_eq!(decoded.dir_bytes, 12);
        assert_eq!(decoded.sections.len(), 3);

        let first = decoded.section(0).expect("section 0");
        assert_eq!(first.table_bytes, 8);
        assert_eq!(first.offsets, vec![8, 10, 10, 14]);
        assert_eq!(first.slots, vec![0, 1, 1, 2]);
        assert_eq!(first.records, vec![vec![1, 2], vec![3, 4, 5, 6], vec![7, 8, 9, 10, 11, 12, 13]]);

        let single = decoded.section(1).expect("section 1");
        assert_eq!(single.offsets, vec![2]);
        assert_eq!(single.slots, vec![0]);
        assert_eq!(single.records, vec![vec![14, 15, 16, 17, 18]]);

        assert_eq!(decoded.section(2).expect("section 2").records.len(), 2);
    }

    #[test]
    fn encode_then_decode_is_byte_identical() {
        let payload = bank();
        let decoded = ScriptBank::decode(&payload).expect("a bank parses");
        assert_eq!(decoded.encode(), payload);
    }

    #[test]
    fn a_json_document_holds_the_records_as_hex() {
        let payload = bank();
        let decoded = ScriptBank::decode(&payload).expect("a bank parses");
        let document: serde_json::Value = serde_json::from_str(&decoded.json()).expect("JSON reads");
        assert_eq!(document["dir_bytes"], 12);
        assert_eq!(document["sections"][0]["offsets"][2], 10);
        assert_eq!(document["sections"][0]["records"][1], "03040506");
        assert_eq!(document["sections"][1]["records"][0], "0e0f101112");
    }

    /// A magic-effect bank: a one-word directory, then one section of two records.
    #[test]
    fn a_one_section_bank_reads_and_encodes_back() {
        let mut payload = Vec::new();
        push_u32_le(&mut payload, 4);
        push_u16_le(&mut payload, 4);
        push_u16_le(&mut payload, 6);
        payload.extend_from_slice(&[1, 2, 3, 4, 5]);

        let decoded = ScriptBank::decode(&payload).expect("a one-section bank parses");
        assert_eq!(decoded.dir_bytes, 4);
        assert_eq!(decoded.sections.len(), 1);
        assert_eq!(decoded.sections[0].records, vec![vec![1, 2], vec![3, 4, 5]]);
        assert_eq!(decoded.encode(), payload);
    }

    #[test]
    fn rejects_a_payload_shorter_than_a_directory() {
        assert!(matches!(ScriptBank::decode(&[1, 2, 3]), Err(Error::ScriptBankShape(_))));
        assert!(matches!(ScriptBank::decode(&[4, 0, 0, 0, 2]), Err(Error::ScriptBankShape(_))));
    }

    #[test]
    fn rejects_a_directory_size_that_is_not_four_l() {
        let mut payload = bank();
        payload[0] = 10; // 10 is not 4*L
        assert!(matches!(ScriptBank::decode(&payload), Err(Error::ScriptBankShape(_))));
    }

    #[test]
    fn rejects_a_section_offset_that_is_not_strictly_increasing() {
        let mut payload = bank();
        payload[4] = 12; // section 1 at the section 0 base
        assert!(matches!(ScriptBank::decode(&payload), Err(Error::ScriptBankShape(_))));
    }

    #[test]
    fn rejects_a_table_size_that_is_not_two_m() {
        let mut payload = bank();
        payload[12] = 9;
        assert!(matches!(ScriptBank::decode(&payload), Err(Error::ScriptBankShape(_))));
    }

    #[test]
    fn rejects_a_record_offset_past_the_section_end() {
        let mut payload = bank();
        payload[42] = 40; // section 2's second offset word runs past 54 - 40 = 14
        assert!(matches!(ScriptBank::decode(&payload), Err(Error::ScriptBankShape(_))));
    }

    #[test]
    fn rejects_offsets_that_repeat_out_of_order() {
        let mut payload = bank();
        payload[16] = 0;
        payload[17] = 0; // section 0's third word drops below its predecessor
        assert!(matches!(ScriptBank::decode(&payload), Err(Error::ScriptBankShape(_))));
    }
}
