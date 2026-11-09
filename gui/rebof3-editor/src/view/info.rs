//! What the archive, the classifier and the format's decoder know about an entry: where it sits, where
//! the game loads it, and the fields its format holds.

use gpui::{AnyElement, Div, div, prelude::*, px};
use psx_formats::sep::Package;
use psx_formats::vab::Bank;
use rebof3_emi::cue::CueTable;
use rebof3_emi::entry::vab_body_index;
use rebof3_emi::lz77;
use rebof3_formats::Format;
use rebof3_formats::clut::clut_block;
use rebof3_formats::encounter::EncounterBlock;
use rebof3_formats::game_tables::name_text;
use rebof3_formats::map::MapBundle;
use rebof3_formats::scene::SceneTable;
use rebof3_formats::script::ScriptTable;
use rebof3_formats::sprite_palettes;
use rebof3_formats::text::discover;
use rebof3_formats::vram::texture_words;

use super::Entry;
use super::map::summary as map_summary;
use crate::theme::{RAISED, READ_ONLY, color};
use crate::widgets::{GAP, GAP_SECTION, GAP_SMALL, mono, strong};

/// Width of the label column, in pixels.
const LABEL_WIDTH: f32 = 140.0;
/// The KSEG0 window a load address names RAM in; any other load argument is a descriptor.
const RAM: std::ops::Range<u32> = 0x8000_0000..0x8020_0000;
/// Colours in one CLUT row of a 4-bit texture.
const ROW_COLOURS: usize = 16;

/// One labelled line of the panel.
type Row = (&'static str, String);

pub fn show(entry: &Entry<'_>) -> AnyElement {
    let format = entry.format();

    let column = div()
        .id("info")
        .flex()
        .flex_col()
        .flex_1()
        .overflow_y_scroll()
        .child(grid(place(entry)))
        .child(strong("Format").mt(px(GAP_SECTION)))
        .child(grid(details(entry)))
        .child(strong("Evidence").mt(px(GAP_SECTION)))
        .child(div().text_color(color(READ_ONLY)).child(format.evidence().to_string()));
    column.into_any_element()
}

fn grid(rows: Vec<Row>) -> Div {
    let mut grid = div().flex().flex_col();
    for (at, (label, value)) in rows.into_iter().enumerate() {
        let striped = at % 2 == 1;
        grid = grid.child(
            div()
                .flex()
                .gap(px(GAP))
                .px(px(GAP_SMALL))
                .py(px(GAP_SMALL / 2.0))
                .when(striped, |row| row.bg(color(RAISED)))
                .child(strong(label).w(px(LABEL_WIDTH)).flex_none())
                .child(mono(value)),
        );
    }
    grid
}

/// Where the entry sits in its archive, and where the game puts it.
fn place(entry: &Entry<'_>) -> Vec<Row> {
    let toc = &entry.archive.entries()[entry.index];
    let original = entry.original().len();
    let current = entry.payload().len();
    let format = entry.format();

    let mut rows = vec![
        ("Archive", entry.archive.path.0.clone()),
        ("Entry", entry.index.to_string()),
        ("Entry type", toc.file_type.to_string()),
        ("Archive offset", format!("{:#X}", toc.offset)),
        ("Size on disc", format!("{original} bytes ({original:#X})")),
        ("Size now", format!("{current} bytes ({current:#X})")),
    ];
    if RAM.contains(&toc.ram_ptr) {
        let end = toc.ram_ptr.saturating_add(current as u32);
        rows.push(("Loads at", format!("{:#010X}..{end:#010X}", toc.ram_ptr)));
    } else {
        rows.push(("Load argument", format!("{:#010X}", toc.ram_ptr)));
    }
    rows.extend([
        ("Format", format.name().to_string()),
        ("State", format.state().label().to_string()),
        ("Spec", format.spec_state().to_string()),
        ("Codec", format.codec_state().to_string()),
    ]);
    rows
}

/// The fields the format's decoder reads, or why it cannot read them.
fn details(entry: &Entry<'_>) -> Vec<Row> {
    let payload = entry.payload();
    let rows = match entry.format() {
        Format::VramUpload => texture(entry.load_arg(), payload),
        Format::ClutBlock => palette(entry.load_arg(), payload),
        Format::VabHeader | Format::VabBody => sound_bank(entry),
        Format::SepPackage => sequences(payload),
        Format::CharacterSpritesLz77 => lz77::decode(payload).map_err(|error| error.to_string()).and_then(|decoded| {
            let mut rows = vec![("Decompressed", format!("{} bytes", decoded.len()))];
            rows.extend(character_sprites(&decoded)?);
            Ok(rows)
        }),
        Format::CharacterSprites => character_sprites(payload),
        Format::AreaMessageTable => Ok(messages(payload, entry.load_arg())),
        // The battle table at `0x800F0800` and the magic-effect table at `0x800C3800` have the area script table's
        // format (`wiki: formats/area`).
        Format::AreaScriptTable | Format::BattleScriptTable | Format::MagicScriptTable => script_table(payload),
        Format::AreaActorDefsTable => actor_defs(payload),
        Format::AreaEncounterTable => encounters(payload),
        Format::AreaMapBundle => MapBundle::decode(payload)
            .map(|bundle| vec![("Sections", map_summary(&bundle))])
            .map_err(|error| error.to_string()),
        Format::AreaCellGrid => super::cells::map_size(entry)
            .map(|(width, height)| vec![("Map size", format!("{width} × {height} cells, from the map bundle"))])
            .ok_or_else(|| "no map bundle in this archive".to_string()),
        Format::CueTable => CueTable::decode(payload)
            .map(|table| vec![("Cue records", table.records().len().to_string())])
            .map_err(|error| error.to_string()),
        Format::SharedResourceTable => resource_groups(payload),
        Format::AreaAuxWindow => aux_window(payload),
        _ => Ok(vec![("Fields", "no decoder reads this format yet".into())]),
    };
    rows.unwrap_or_else(|error| vec![("Decoder", format!("does not read this payload: {error}"))])
}

fn texture(load_arg: u32, payload: &[u8]) -> Result<Vec<Row>, String> {
    let words = texture_words(load_arg, payload).map_err(|error| error.to_string())?;
    Ok(vec![
        ("VRAM position", format!("x {} y {}", words.x, words.y)),
        ("VRAM size", format!("{} words × {} rows", words.width, words.height)),
        ("As texels", format!("{} at 4 bpp, {} at 8 bpp, {} at 16 bpp", words.width * 4, words.width * 2, words.width)),
    ])
}

fn palette(load_arg: u32, payload: &[u8]) -> Result<Vec<Row>, String> {
    let bank = clut_block(load_arg, payload).map_err(|error| error.to_string())?;
    let mut rows =
        vec![("Colours", bank.words.len().to_string()), ("4-bit rows", (bank.words.len() / ROW_COLOURS).to_string())];
    if let Some(clut) = bank.vram {
        rows.push(("VRAM position", format!("x {} y {}", clut.x, clut.y)));
    }
    Ok(rows)
}

/// The header and body entries of the bank, and what the header declares.
fn sound_bank(entry: &Entry<'_>) -> Result<Vec<Row>, String> {
    let entries = entry.archive.entries();
    let (header, body) = match entry.format() {
        Format::VabHeader => (entry.index, vab_body_index(entries, entry.index).map_err(|error| error.to_string())?),
        _ => (0..entries.len())
            .filter(|&header| entry.archive.formats[header] == Format::VabHeader)
            .find(|&header| vab_body_index(entries, header).ok() == Some(entry.index))
            .map(|header| (header, entry.index))
            .ok_or("no header in this archive names this body")?,
    };
    let bank = Bank::parse(entry.sibling(header), entry.sibling(body)).map_err(|error| error.to_string())?;
    Ok(vec![
        ("Header entry", header.to_string()),
        ("Body entry", body.to_string()),
        ("Bank id", bank.id.to_string()),
        ("Programs", bank.program_count.to_string()),
        ("Tones", bank.tone_count.to_string()),
        ("Samples", bank.vag_count.to_string()),
        ("Master volume", bank.master_vol.to_string()),
        ("Master pan", bank.master_pan.to_string()),
    ])
}

fn sequences(payload: &[u8]) -> Result<Vec<Row>, String> {
    let package = Package::parse(payload).map_err(|error| error.to_string())?;
    let ids: Vec<String> = package.sequences.iter().map(|sequence| sequence.seq_id.to_string()).collect();
    Ok(vec![("Sequences", package.sequences.len().to_string()), ("Sequence ids", ids.join(", "))])
}

fn character_sprites(payload: &[u8]) -> Result<Vec<Row>, String> {
    let bank = sprite_palettes::parse(payload).map_err(|error| error.to_string())?;
    let mut rows = vec![("Sections", bank.sections.len().to_string())];
    for section in &bank.sections {
        rows.push((
            "Section",
            format!(
                "at {:#X}, palette table at {:#X}, {}",
                section.start,
                section.table,
                count(section.palettes.len(), "palette")
            ),
        ));
    }
    Ok(rows)
}

/// `count` things, as `1 palette` or `3 palettes`.
fn count(count: usize, thing: &str) -> String {
    if count == 1 { format!("1 {thing}") } else { format!("{count} {thing}s") }
}

/// The bank's messages, or the strings a scan finds when the bank has no offset table.
fn messages(payload: &[u8], load_arg: u32) -> Vec<Row> {
    match discover::table_records(payload) {
        Some(records) => vec![("Messages", records.len().to_string())],
        None => vec![
            ("Messages", "no offset table".into()),
            ("Strings found", discover::strings(payload, Some(load_arg)).len().to_string()),
        ],
    }
}

fn script_table(payload: &[u8]) -> Result<Vec<Row>, String> {
    let table = ScriptTable::decode(payload).map_err(|error| error.to_string())?;
    let records: usize = table.sections().iter().map(|section| section.records.len()).sum();
    Ok(vec![
        ("Directory", format!("{} bytes", table.dir_bytes)),
        ("Sections", table.sections().len().to_string()),
        ("Scripts", records.to_string()),
    ])
}

/// The module id that opens a window loaded at `0x80117000`. The nine retail windows hold ids `0xFC` to
/// `0x104`; area049's window is code whose functions sit in the game's callback table, and the other eight
/// hold data whose layout is not traced.
fn aux_window(payload: &[u8]) -> Result<Vec<Row>, String> {
    const MODULE_ID: usize = 4;

    let id = payload.get(..MODULE_ID).ok_or("shorter than the module id word")?;
    let id = u32::from_le_bytes(id.try_into().expect("the slice is one word"));
    Ok(vec![("Module id", format!("{id:#X}")), ("Body", format!("{} bytes", payload.len() - MODULE_ID))])
}

/// The id groups of a payload loaded at `0x8001A000`. `Resource_GetText` reads base word `id >> 14`; each
/// base is a group's offset, and the group opens with a `u16` slot table (`wiki: data/text#references`).
/// The groups follow the base words, so the first base also says how many base words there are.
fn resource_groups(payload: &[u8]) -> Result<Vec<Row>, String> {
    const BASE_WORD: usize = 4;
    const SLOT: usize = 2;
    const LABELS: [&str; 4] =
        ["Group 0 (ids 0x0000)", "Group 1 (ids 0x4000)", "Group 2 (ids 0x8000)", "Group 3 (ids 0xC000)"];

    let word =
        |at: usize| payload.get(at..at + BASE_WORD).and_then(|bytes| bytes.try_into().ok()).map(u32::from_le_bytes);
    let half = |at: usize| payload.get(at..at + SLOT).and_then(|bytes| bytes.try_into().ok()).map(u16::from_le_bytes);

    let first = word(0).ok_or("no base words")? as usize;
    let groups = first / BASE_WORD;
    if groups == 0 || groups > LABELS.len() || !first.is_multiple_of(BASE_WORD) {
        return Err(format!("the first group starts at 0x{first:X}, not after one to four base words"));
    }

    let mut rows = vec![("Groups", groups.to_string())];
    for (group, label) in LABELS.iter().enumerate().take(groups) {
        let base = word(group * BASE_WORD).ok_or("a base word is past the payload")? as usize;
        let table = half(base).ok_or_else(|| format!("group {group} at 0x{base:X} is past the payload"))?;
        rows.push((label, format!("at 0x{base:X}, {} slots", usize::from(table) / SLOT)));
    }
    Ok(rows)
}

fn actor_defs(payload: &[u8]) -> Result<Vec<Row>, String> {
    let table = SceneTable::decode(payload).map_err(|error| error.to_string())?;
    let sections: Vec<String> = table.sections().iter().map(u8::to_string).collect();
    Ok(vec![("Actor definitions", table.records().len().to_string()), ("Script sections", sections.join(", "))])
}

/// The encounters that can happen, and the enemies the block holds.
pub fn encounters(payload: &[u8]) -> Result<Vec<Row>, String> {
    let block = EncounterBlock::parse(payload).map_err(|error| error.to_string())?;
    let live = block.encounters.iter().filter(|encounter| encounter.appearance_rate != 0).count();
    let enemies: Vec<String> =
        block.enemies.iter().map(|enemy| name_text(&enemy.name)).filter(|name| !name.is_empty()).collect();
    Ok(vec![
        ("Encounters", format!("{live} of {} with a nonzero rate", block.encounters.len())),
        ("Enemies", enemies.join(", ")),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use rebof3_formats::encounter::{BLOCK_BYTES, ENCOUNTER_BYTES, ENEMY_BASE};

    #[test]
    fn a_resource_table_lists_the_groups_after_its_base_words() {
        // Two base words, so two groups: group 0 at 0x08 with 2 slots, group 1 at 0x10 with 1 slot.
        let mut payload = vec![0; 0x14];
        payload[0..4].copy_from_slice(&0x08u32.to_le_bytes());
        payload[4..8].copy_from_slice(&0x10u32.to_le_bytes());
        payload[0x08..0x0A].copy_from_slice(&4u16.to_le_bytes());
        payload[0x10..0x12].copy_from_slice(&2u16.to_le_bytes());

        let rows = resource_groups(&payload).unwrap();
        assert_eq!(rows[0], ("Groups", "2".into()));
        assert_eq!(rows[1].1, "at 0x8, 2 slots");
        assert_eq!(rows[2].1, "at 0x10, 1 slots");
        assert!(resource_groups(&[0x30, 0, 0, 0]).is_err(), "twelve base words is not a table");
    }

    #[test]
    fn an_aux_window_shows_its_module_id() {
        let rows = aux_window(&[0x04, 0x01, 0, 0, 0xAA, 0xBB]).unwrap();
        assert_eq!(rows[0], ("Module id", "0x104".into()));
        assert_eq!(rows[1], ("Body", "2 bytes".into()));
        assert!(aux_window(&[1, 2]).is_err());
    }

    #[test]
    fn an_encounter_block_lists_its_live_encounters_and_named_enemies() {
        let mut block = vec![0; BLOCK_BYTES];
        block[ENCOUNTER_BYTES - 1] = 4;
        block[ENEMY_BASE..ENEMY_BASE + 6].copy_from_slice(b"Worker");

        let rows = encounters(&block).unwrap();
        assert_eq!(rows[0], ("Encounters", "1 of 8 with a nonzero rate".into()));
        assert_eq!(rows[1], ("Enemies", "Worker".into()));
        assert!(encounters(&block[..10]).is_err());
    }

    #[test]
    fn character_sprites_lists_its_sections() {
        // One bare section: the table offset at +4 points at a table of one palette.
        let mut section = vec![0; 8];
        section[4] = 8;
        section.extend(4u32.to_le_bytes());
        section.extend([0; sprite_palettes::PALETTE_BYTES]);

        let rows = character_sprites(&section).unwrap();
        assert_eq!(rows[0], ("Sections", "1".into()));
        assert!(rows[1].1.ends_with("1 palette"), "{rows:?}");
    }

    /// Every entry of a whole extracted disc gets its format's fields without a decoder error. Point
    /// `BOF3_DISC_DIR` at an extracted disc; without it the test skips.
    #[test]
    fn every_retail_entry_shows_its_fields() {
        let Some(disc) = crate::retail_support::retail_disc() else { return };
        let folder = std::env::temp_dir().join(format!("bof3-edit-info-{}", std::process::id()));
        let mut project = crate::project::Project::open(disc.as_ref(), &folder).expect("project opens");

        let mut failures = Vec::new();
        let mut shown = 0;
        for path in project.archives.clone() {
            let archive = project.load(&path).expect("archive loads");
            for index in 0..archive.entries().len() {
                let entry = Entry { project: &project, archive: &archive, index };
                shown += 1;
                let Some((_, error)) = details(&entry).into_iter().find(|(label, _)| *label == "Decoder") else {
                    continue;
                };
                // Three raw character sprites hold no palette table at their section's +4 word.
                if entry.format() == Format::CharacterSprites && error.contains("is not a character sprites") {
                    continue;
                }
                failures.push(format!("{}#{index} {:?}: {error}", path.0, entry.format()));
            }
        }

        let _ = std::fs::remove_dir_all(&folder);
        eprintln!("info: {} of {shown} entries fail to decode", failures.len());
        assert!(failures.is_empty(), "{:?}", failures.iter().take(8).collect::<Vec<_>>());
    }
}
