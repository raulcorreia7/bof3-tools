//! Sprite banks: the palette layer of a character bank.
//!
//! A bank is a container of three sections (its first word is [`CONTAINER_HEAD`]: `BPLCHAR`, `PLCHAR` and
//! `ETC/START.EMI` entry 7 at `0x8003B800`), or one section by itself (the section entries at `0x8003B80C` and its
//! siblings). A section's word at +4 is the offset of its palette table, relative to the section. The table is a
//! count of palettes written as offsets: the first word is `4n` for `n` palettes, and palette `k` follows the offsets
//! at `4n + 0x40k`. A palette holds [`COLOURS`] 16-bit colours.
//!
//! The rest of a section is its node data, which is not decoded. [`export`] writes the palettes only, and [`import`]
//! writes them back over the bank's own bytes, so every other byte stays as it was.
//!
//! The palette index is the `mode` argument of `GFX_StagePalette` (`decomp/src/engine/slus/GFX_StagePalette.c`), which
//! copies palette `mode` into the palette RAM of a work slot. The section is chosen by `Party_LoadMemberSprite`, which stages
//! the record at work `+0x4C`. Its callers pass `mode` 0, except `func_8019C664`, which passes 1.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// The first word of a container: the offset of its first section, which is also the header's size.
pub const CONTAINER_HEAD: u32 = 0x0C;
/// Sections in a container.
pub const CONTAINER_SECTIONS: usize = 3;
/// 16-bit colours in one palette.
pub const COLOURS: usize = 0x20;
/// Bytes in one palette.
pub const PALETTE_BYTES: usize = COLOURS * 2;
/// Bytes in one word of a palette table: a count-derived offset.
const WORD: usize = 4;
/// The section word that holds the offset of the section's palette table.
const TABLE_AT: usize = 4;

/// One palette: 16-bit colours in CLUT order.
pub type Palette = [u16; COLOURS];

/// One section's palettes, and where its palette table sits in the payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Section {
    /// The payload offset of the section.
    pub start: usize,
    /// The payload offset of the palette table: its offset words, then the palettes.
    pub table: usize,
    /// The palettes, in table order.
    pub palettes: Vec<Palette>,
}

impl Section {
    /// The payload offset of palette `index`.
    pub fn palette_at(&self, index: usize) -> usize {
        self.table + WORD * self.palettes.len() + PALETTE_BYTES * index
    }

    /// The bytes of the section's palette table: its offset words and its palettes.
    fn table_bytes(&self) -> usize {
        self.palettes.len() * (WORD + PALETTE_BYTES)
    }
}

/// A bank's sections, in container order. A bare section is a bank of one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Bank {
    pub sections: Vec<Section>,
}

impl Bank {
    /// The bytes of the palette layer: every section's palette table.
    pub fn table_bytes(&self) -> usize {
        self.sections.iter().map(Section::table_bytes).sum()
    }
}

/// The JSON of a bank's palettes: `{"sections": [{"palettes": [[colour, ...], ...]}, ...]}`, with each colour as four
/// lowercase hex digits.
#[derive(Serialize, Deserialize)]
struct Json {
    sections: Vec<JsonSection>,
}

#[derive(Serialize, Deserialize)]
struct JsonSection {
    palettes: Vec<Vec<String>>,
}

/// The bank in `payload`: a container when its first word is [`CONTAINER_HEAD`], otherwise one section at offset 0.
pub fn parse(payload: &[u8]) -> Result<Bank> {
    let first = word(payload, 0)?;
    if first != CONTAINER_HEAD {
        return Ok(Bank { sections: vec![section(payload, 0, payload.len())?] });
    }

    let mut starts = Vec::with_capacity(CONTAINER_SECTIONS);
    starts.push(CONTAINER_HEAD as usize);
    for index in 1..CONTAINER_SECTIONS {
        let offset = word(payload, index * WORD)?;
        starts.push(usize::try_from(offset).map_err(|_| {
            Error::CharacterBank(format!("container section offset {offset:#x} does not fit in memory"))
        })?);
    }

    let mut sections = Vec::with_capacity(CONTAINER_SECTIONS);
    for (index, start) in starts.iter().copied().enumerate() {
        let end = starts.get(index + 1).copied().unwrap_or(payload.len());
        if start < CONTAINER_HEAD as usize || start >= end || end > payload.len() {
            return Err(not_a_bank(start, "its container section bounds are invalid"));
        }
        sections.push(section(payload, start, end)?);
    }
    Ok(Bank { sections })
}

/// The JSON of the palettes of the bank in `payload`.
pub fn export(payload: &[u8]) -> Result<Vec<u8>> {
    let bank = parse(payload)?;
    let json = Json {
        sections: bank
            .sections
            .iter()
            .map(|section| JsonSection {
                palettes: section
                    .palettes
                    .iter()
                    .map(|palette| palette.iter().map(|colour| format!("{colour:04x}")).collect())
                    .collect(),
            })
            .collect(),
    };
    Ok(serde_json::to_vec_pretty(&json).expect("a palette serializes"))
}

/// The payload with its palettes replaced by the JSON that [`export`] wrote. The JSON must have the bank's sections
/// and the same palette count in each; every byte outside the palettes stays as it was.
pub fn import(json: &[u8], payload: &[u8]) -> Result<Vec<u8>> {
    let json: Json = serde_json::from_slice(json)
        .map_err(|error| Error::CharacterBank(format!("the palettes JSON does not read: {error}")))?;
    let bank = parse(payload)?;
    if json.sections.len() != bank.sections.len() {
        return Err(Error::CharacterBank(format!(
            "{} sections; the bank has {}",
            json.sections.len(),
            bank.sections.len()
        )));
    }
    let mut out = payload.to_vec();
    for (index, (section, rows)) in bank.sections.iter().zip(&json.sections).enumerate() {
        if rows.palettes.len() != section.palettes.len() {
            return Err(Error::CharacterBank(format!(
                "section {index} has {} palettes; the bank has {}",
                rows.palettes.len(),
                section.palettes.len()
            )));
        }
        for (palette, row) in rows.palettes.iter().enumerate() {
            if row.len() != COLOURS {
                return Err(Error::CharacterBank(format!(
                    "section {index} palette {palette} has {} colours; a palette has {COLOURS}",
                    row.len()
                )));
            }
            let at = section.palette_at(palette);
            for (colour, text) in row.iter().enumerate() {
                let value = colour_value(text)?;
                out[at + 2 * colour..at + 2 * colour + 2].copy_from_slice(&value.to_le_bytes());
            }
        }
    }
    Ok(out)
}

/// The section at `start`. Its palette table must be in the layout of the module documentation.
fn section(payload: &[u8], start: usize, end: usize) -> Result<Section> {
    let outside = |at: usize| not_a_bank(start, format!("the word at {at:#x} is past the end of the section"));
    let read = |at: usize| {
        let word_end = at.checked_add(WORD).ok_or_else(|| outside(at))?;
        if at < start || word_end > end {
            return Err(outside(at));
        }
        word(payload, at).map_err(|_| outside(at))
    };
    let table_word = start.checked_add(TABLE_AT).ok_or_else(|| outside(start))?;
    let table_offset = usize::try_from(read(table_word)?)
        .map_err(|_| not_a_bank(start, "its palette-table offset does not fit in memory"))?;
    let table =
        start.checked_add(table_offset).ok_or_else(|| not_a_bank(start, "its palette-table offset overflows"))?;
    let first = usize::try_from(read(table)?)
        .map_err(|_| not_a_bank(start, "its palette-table size does not fit in memory"))?;
    if first == 0 || !first.is_multiple_of(WORD) {
        return Err(not_a_bank(start, format!("its palette table starts with {first:#x}")));
    }
    let count = first / WORD;
    let table_end = table.checked_add(first).ok_or_else(|| not_a_bank(start, "its palette table overflows"))?;
    if table_end > end {
        return Err(not_a_bank(start, "its palette offsets run past the end of the section"));
    }

    for index in 0..count {
        let offset_at = WORD
            .checked_mul(index)
            .and_then(|relative| table.checked_add(relative))
            .ok_or_else(|| not_a_bank(start, "its palette offset table overflows"))?;
        let offset = usize::try_from(read(offset_at)?)
            .map_err(|_| not_a_bank(start, "a palette offset does not fit in memory"))?;
        let expected = PALETTE_BYTES
            .checked_mul(index)
            .and_then(|bytes| first.checked_add(bytes))
            .ok_or_else(|| not_a_bank(start, "a palette offset overflows"))?;
        if offset != expected {
            return Err(not_a_bank(start, format!("palette {index} is at {offset:#x}, not {expected:#x}")));
        }
        let palette_at = table.checked_add(offset).ok_or_else(|| not_a_bank(start, "a palette offset overflows"))?;
        let palette_end =
            palette_at.checked_add(PALETTE_BYTES).ok_or_else(|| not_a_bank(start, "a palette range overflows"))?;
        if palette_end > end {
            return Err(not_a_bank(start, format!("palette {index} runs past the end of the section")));
        }
    }

    let mut palettes = Vec::with_capacity(count);
    for index in 0..count {
        let relative = PALETTE_BYTES
            .checked_mul(index)
            .and_then(|bytes| first.checked_add(bytes))
            .ok_or_else(|| not_a_bank(start, "a palette offset overflows"))?;
        let at = table.checked_add(relative).ok_or_else(|| not_a_bank(start, "a palette offset overflows"))?;
        palettes.push(palette(payload, at)?);
    }
    Ok(Section { start, table, palettes })
}

/// The palette at `at`.
fn palette(payload: &[u8], at: usize) -> Result<Palette> {
    let end = at
        .checked_add(PALETTE_BYTES)
        .ok_or_else(|| Error::CharacterBank(format!("the palette at {at:#x} runs past the end of the payload")))?;
    let bytes = payload
        .get(at..end)
        .ok_or_else(|| Error::CharacterBank(format!("the palette at {at:#x} runs past the end of the payload")))?;
    let mut palette = [0u16; COLOURS];
    for (colour, pair) in palette.iter_mut().zip(bytes.as_chunks::<2>().0) {
        *colour = u16::from_le_bytes(*pair);
    }
    Ok(palette)
}

/// The little-endian word at `at`.
fn word(payload: &[u8], at: usize) -> Result<u32> {
    let end = at
        .checked_add(WORD)
        .ok_or_else(|| Error::CharacterBank(format!("the payload ends before the word at {at:#x}")))?;
    payload
        .get(at..end)
        .map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        .ok_or_else(|| Error::CharacterBank(format!("the payload ends before the word at {at:#x}")))
}

/// A colour from its four hex digits.
fn colour_value(text: &str) -> Result<u16> {
    if text.len() != 4 || !text.bytes().all(|digit| digit.is_ascii_hexdigit()) {
        return Err(Error::CharacterBank(format!("{text:?} is not a colour: four hex digits")));
    }
    u16::from_str_radix(text, 16).map_err(|_| Error::CharacterBank(format!("{text:?} is not a colour")))
}

fn not_a_bank(at: usize, why: impl core::fmt::Display) -> Error {
    Error::CharacterBank(format!("the section at {at:#x} is not a sprite bank: {why}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A section with `count` palettes and 0x20 bytes of node data after them, whose colours begin at `seed`.
    fn section_bytes(count: usize, seed: u16) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend(0x10u32.to_le_bytes());
        out.extend(0x10u32.to_le_bytes());
        out.extend([0u8; 8]);
        for index in 0..count {
            out.extend(((WORD * count + PALETTE_BYTES * index) as u32).to_le_bytes());
        }
        for colour in 0..count * COLOURS {
            out.extend((seed + colour as u16).to_le_bytes());
        }
        out.extend([0xA5u8; 0x20]);
        out
    }

    /// A container whose three sections hold `counts` palettes each.
    fn container(counts: [usize; CONTAINER_SECTIONS]) -> Vec<u8> {
        let sections: Vec<Vec<u8>> = counts
            .iter()
            .enumerate()
            .map(|(index, &count)| section_bytes(count, 0x1000 * (index as u16 + 1)))
            .collect();
        let mut out = Vec::new();
        let mut start = CONTAINER_HEAD as usize;
        out.extend(CONTAINER_HEAD.to_le_bytes());
        for section in &sections[..CONTAINER_SECTIONS - 1] {
            start += section.len();
            out.extend((start as u32).to_le_bytes());
        }
        for section in &sections {
            out.extend(section);
        }
        out
    }

    #[test]
    fn parses_a_container_with_its_palette_counts() {
        let payload = container([1, 2, 8]);
        let bank = parse(&payload).expect("a container parses");
        let counts: Vec<usize> = bank.sections.iter().map(|section| section.palettes.len()).collect();
        assert_eq!(counts, [1, 2, 8]);
        assert_eq!(bank.sections[2].palettes[7][31], 0x3000 + 7 * COLOURS as u16 + 31);
        assert_eq!(bank.table_bytes(), 68 * 11);
    }

    #[test]
    fn parses_a_bare_section_as_a_bank_of_one() {
        let payload = section_bytes(2, 0x10);
        let bank = parse(&payload).expect("a section parses");
        assert_eq!(bank.sections.len(), 1);
        assert_eq!(bank.sections[0].start, 0);
        assert_eq!(bank.sections[0].palettes[1][0], 0x10 + COLOURS as u16);
    }

    #[test]
    fn export_then_import_is_byte_identical() {
        let payload = container([1, 2, 8]);
        let json = export(&payload).expect("exports");
        assert_eq!(import(&json, &payload).expect("imports"), payload);
    }

    #[test]
    fn import_changes_only_the_edited_colour() {
        let payload = container([1, 2, 8]);
        let mut json: serde_json::Value = serde_json::from_slice(&export(&payload).expect("exports")).expect("reads");
        json["sections"][1]["palettes"][1][5] = serde_json::Value::from("beef");
        let edited = import(json.to_string().as_bytes(), &payload).expect("imports");

        let bank = parse(&payload).expect("parses");
        let at = bank.sections[1].palette_at(1) + 2 * 5;
        let changed: Vec<usize> = (0..payload.len()).filter(|&byte| payload[byte] != edited[byte]).collect();
        assert_eq!(changed, [at, at + 1]);
        assert_eq!(&edited[at..at + 2], &0xBEEFu16.to_le_bytes());
    }

    #[test]
    fn import_rejects_a_changed_palette_count() {
        let payload = container([1, 2, 8]);
        let mut json: serde_json::Value = serde_json::from_slice(&export(&payload).expect("exports")).expect("reads");
        json["sections"][0]["palettes"].as_array_mut().expect("an array").push(serde_json::json!([]));
        let error = import(json.to_string().as_bytes(), &payload).expect_err("a count change is refused");
        assert!(error.to_string().contains("section 0 has 2 palettes; the bank has 1"), "{error}");
    }

    #[test]
    fn import_rejects_a_colour_that_is_not_hex() {
        let payload = container([1, 1, 1]);
        let mut json: serde_json::Value = serde_json::from_slice(&export(&payload).expect("exports")).expect("reads");
        json["sections"][0]["palettes"][0][0] = serde_json::Value::from("zzzz");
        assert!(import(json.to_string().as_bytes(), &payload).is_err());
    }

    #[test]
    fn rejects_a_palette_out_of_the_layout() {
        let mut payload = container([2, 1, 1]);
        // Corrupt the second palette offset of section 0: the table starts 0x10 into the section, at 0x0C + 0x10.
        let offset_word = CONTAINER_HEAD as usize + 0x10 + WORD;
        payload[offset_word..offset_word + WORD].copy_from_slice(&0x99u32.to_le_bytes());
        assert!(parse(&payload).is_err());
    }
    #[test]
    fn rejects_a_palette_that_crosses_its_section_boundary() {
        let mut payload = container([1, 1, 1]);
        let section_start = CONTAINER_HEAD as usize;
        let second_start = word(&payload, WORD).expect("the second section starts") as usize;
        let table = second_start - WORD;
        let table_offset = (table - section_start) as u32;
        payload[section_start + TABLE_AT..section_start + TABLE_AT + WORD].copy_from_slice(&table_offset.to_le_bytes());
        payload[table..table + WORD].copy_from_slice(&(WORD as u32).to_le_bytes());
        assert!(parse(&payload).is_err());
    }
}
