//! Record layouts the editor shows as spreadsheet columns.
//!
//! Each column reads and writes the record bytes directly, at the offsets the decompilation's
//! headers give (`include/game/item.h`, `ability.h`, `enemy.h` and `character.h`), so a column edit
//! changes exactly the bytes it names and every byte no column names keeps its stored value.

use rebof3_formats::Format;
use rebof3_formats::encounter::{
    self, ENCOUNTER_BYTES, ENCOUNTER_ENEMIES, ENCOUNTERS, ENEMIES, ENEMY_BASE, ENEMY_BYTES,
};
use rebof3_formats::game_tables as tables;

/// The archive that holds the fixed game tables in entry 0.
pub const GAME: &str = "BIN/ETC/GAME.EMI";
/// The archive and entry that hold the party members' starting records.
pub const START: &str = "BIN/ETC/START.EMI";
pub const START_PARTY_ENTRY: usize = 8;

/// The item category that marks an empty slot (`ITEM_CAT_EMPTY`).
pub const EMPTY_CATEGORY: u8 = 0xFF;
/// An enemy slot of an encounter that holds no enemy.
pub const EMPTY_ENEMY: u8 = 0xFF;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Field {
    /// An inline name in the name encoding, this many bytes wide.
    Name(usize),
    U8,
    U16,
    U32,
    /// `bits` bits of a byte, from bit `shift` up.
    Part {
        shift: u8,
        bits: u8,
    },
    /// A byte of flag bits, bit 0 first.
    Bits(&'static [&'static str]),
    /// A byte that picks one of a few values.
    Choice(&'static [(u8, &'static str)]),
    /// An item index whose category is the byte at the given record offset.
    ItemAt(usize),
    /// An item index into one category's table.
    ItemOf(u8),
    /// An ability index into the ability table.
    Ability,
    /// An enemy index into the enemy records of the same encounter block.
    Enemy,
}

#[derive(Clone, Copy, Debug)]
pub struct Column {
    pub name: &'static str,
    pub offset: usize,
    pub field: Field,
}

const fn col(name: &'static str, offset: usize, field: Field) -> Column {
    Column { name, offset, field }
}

/// The high and the low nibble of a byte.
const HIGH_NIBBLE: Field = Field::Part { shift: 4, bits: 4 };
const LOW_NIBBLE: Field = Field::Part { shift: 0, bits: 4 };

/// `ItemFlags`.
const ITEM_FLAGS: &[&str] = &[
    "target both",
    "target select",
    "enemy default",
    "target all",
    "story item",
    "show name",
    "show animation",
    "menu use",
];
/// `ElementFlags`, the low byte the weapon and ability masks hold.
const ELEMENTS: &[&str] = &["fire", "frost", "electric", "earth", "wind", "holy", "psionic", "status"];
/// `AbilityFlags`.
const ABILITY_FLAGS: &[&str] =
    &["death", "examinable", "silenceable", "physical", "affects stats", "bit 5", "bit 6", "bit 7"];
/// `AbilityTargetFlags`.
const ABILITY_TARGETS: &[&str] =
    &["field", "battle", "no hit reaction", "bit 3", "all", "opponent", "select", "both sides"];
/// `EquipFlags`: the party members who may equip the item.
const PARTY: &[&str] = &["Ryu", "Nina", "Garr", "Teepo", "Rei", "Momo", "Peco", "Whelp"];
/// The low byte of `StatusFlags`.
const STATUS: &[&str] = &["bit 0", "bit 1", "paralyzed", "blind", "silenced", "confused", "sleeping", "poisoned"];
/// `ArmorType`: the equip slot an armor record fits.
const ARMOR_TYPES: &[(u8, &str)] = &[(0, "nothing"), (2, "shield"), (3, "helmet"), (4, "body")];
/// `ItemCategory`.
const ITEM_CATEGORIES: &[(u8, &str)] = &[
    (CATEGORY_ITEM, "item"),
    (CATEGORY_WEAPON, "weapon"),
    (CATEGORY_ARMOR, "armor"),
    (CATEGORY_ACCESSORY, "accessory"),
    (CATEGORY_VITAL, "vital"),
    (EMPTY_CATEGORY, "empty"),
];
/// The ability classes, in `AbilitySet` order.
const ABILITY_CLASSES: &[(u8, &str)] = &[(0, "healing"), (1, "assist"), (2, "attack"), (3, "skill")];
/// `EnemyFamily`, the high byte of an enemy id.
const ENEMY_FAMILIES: &[(u8, &str)] = &[(0, "none"), (1, "dragon"), (4, "demon")];
/// `MasterId`.
const MASTERS: &[(u8, &str)] = &[
    (0, "Bunyan"),
    (1, "Mygas"),
    (2, "Yggdrasil"),
    (3, "D'Lonzo"),
    (4, "Fahl"),
    (5, "Durandal"),
    (6, "Giotto"),
    (7, "Hondara"),
    (8, "Emitai"),
    (9, "Deis"),
    (10, "Hachio"),
    (11, "Bais"),
    (12, "Lang"),
    (13, "Lee"),
    (14, "Wynn"),
    (15, "Ladon"),
    (16, "Meryleep"),
    (0xFF, "none"),
];

/// The item categories, as `Item_GetRecord` resolves them.
const CATEGORY_ITEM: u8 = 0;
const CATEGORY_WEAPON: u8 = 1;
const CATEGORY_ARMOR: u8 = 2;
const CATEGORY_ACCESSORY: u8 = 3;
const CATEGORY_VITAL: u8 = 4;

/// The tables each item category indexes, in category order.
const CATEGORY_TABLES: [tables::Table; 5] =
    [tables::ITEMS, tables::WEAPONS, tables::ARMOR, tables::ACCESSORIES, tables::VITAL_ITEMS];

/// Where a sheet's records live.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Place {
    /// One entry of one archive.
    Entry { archive: &'static str, entry: usize },
    /// Every entry of this format, in any archive.
    Format(Format),
}

/// How many records a sheet holds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Count {
    Fixed(usize),
    /// As many whole records as the payload holds after the first.
    Payload,
    /// The `u32` at this payload offset.
    Word(usize),
    /// The `u16` at payload offset `at`, times `per`.
    Blocks {
        at: usize,
        per: usize,
    },
}

/// A table of fixed-size records: where it lives, its first record's payload offset, and its columns.
pub struct Table {
    pub name: &'static str,
    pub place: Place,
    pub start: usize,
    pub count: Count,
    pub record: usize,
    pub columns: Vec<Column>,
}

impl Table {
    /// The payload bytes of record `row`.
    pub fn range(&self, row: usize) -> std::ops::Range<usize> {
        let start = self.start + row * self.record;
        start..start + self.record
    }

    /// The records the sheet holds in `payload`.
    pub fn rows(&self, payload: &[u8]) -> usize {
        match self.count {
            Count::Fixed(count) => count,
            Count::Payload => payload.len().saturating_sub(self.start) / self.record,
            Count::Word(at) => payload
                .get(at..at + 4)
                .map_or(0, |word| u32::from_le_bytes(word.try_into().expect("four bytes")) as usize),
            Count::Blocks { at, per } => payload
                .get(at..at + 2)
                .map_or(0, |half| usize::from(u16::from_le_bytes(half.try_into().expect("two bytes"))) * per),
        }
    }

    /// The payload offset just past the last record in `payload`.
    pub fn end(&self, payload: &[u8]) -> usize {
        self.start + self.rows(payload) * self.record
    }

    /// Whether the sheet's records live in entry `entry` of `archive`, whose format is `format`.
    pub fn applies(&self, archive: &str, entry: usize, format: Format) -> bool {
        match self.place {
            Place::Entry { archive: at, entry: index } => at == archive && index == entry,
            Place::Format(of) => of == format,
        }
    }
}

fn game(table: tables::Table, columns: Vec<Column>) -> Table {
    Table {
        name: table.name,
        place: Place::Entry { archive: GAME, entry: 0 },
        start: table.archive_offset - tables::PAYLOAD_START,
        count: Count::Fixed(table.count),
        record: table.record,
        columns,
    }
}

/// The sheets of entry `entry` of `archive`, whose format is `format`.
pub fn sheets_for(archive: &str, entry: usize, format: Format) -> Vec<Table> {
    sheets().into_iter().filter(|sheet| sheet.applies(archive, entry, format)).collect()
}

/// Every sheet the editor knows.
pub fn sheets() -> Vec<Table> {
    use Field::*;
    let name = Name(tables::NAME_BYTES);
    vec![
        game(
            tables::ITEMS,
            vec![
                col("name", 0x00, name),
                col("flags", 0x0c, Bits(ITEM_FLAGS)),
                col("icon", 0x0d, U8),
                col("description", 0x0e, U16),
                col("price", 0x10, U16),
            ],
        ),
        game(tables::VITAL_ITEMS, vec![col("name", 0x00, name), col("description", 0x0c, U16)]),
        game(
            tables::WEAPONS,
            vec![
                col("name", 0x00, name),
                col("equip", 0x0c, Bits(PARTY)),
                col("flags", 0x0d, U8),
                col("icon", 0x0e, U8),
                col("element", 0x0f, Bits(ELEMENTS)),
                col("weight", 0x10, U8),
                col("sound", 0x11, U8),
                col("power", 0x12, U8),
                col("description", 0x14, U16),
                col("price", 0x16, U16),
            ],
        ),
        game(
            tables::ARMOR,
            vec![
                col("name", 0x00, name),
                col("equip", 0x0c, Bits(PARTY)),
                col("flags", 0x0d, U8),
                col("equip slot", 0x0e, Choice(ARMOR_TYPES)),
                col("weight", 0x0f, U8),
                col("power", 0x10, U8),
                col("description", 0x12, U16),
                col("price", 0x14, U16),
            ],
        ),
        game(
            tables::ACCESSORIES,
            vec![
                col("name", 0x00, name),
                col("equip", 0x0c, Bits(PARTY)),
                col("flags", 0x0d, U8),
                col("icon", 0x0e, U8),
                col("weight", 0x0f, U8),
                col("description", 0x10, U16),
                col("price", 0x12, U16),
            ],
        ),
        game(tables::SHOPS, shop_columns()),
        game(
            tables::ABILITIES,
            vec![
                col("name", 0x00, name),
                col("targeting", 0x0c, Bits(ABILITY_TARGETS)),
                col("class", 0x0d, Choice(ABILITY_CLASSES)),
                col("cost", 0x0e, U8),
                col("power", 0x0f, U8),
                col("element", 0x10, Bits(ELEMENTS)),
                col("flags", 0x11, Bits(ABILITY_FLAGS)),
                col("description", 0x12, U16),
            ],
        ),
        game(
            tables::LEVEL_GROWTH,
            vec![
                col("exp", 0x00, U16),
                col("hp", 0x02, U8),
                col("ap", 0x03, U8),
                col("power", 0x04, HIGH_NIBBLE),
                col("defense", 0x04, LOW_NIBBLE),
                col("agility", 0x05, HIGH_NIBBLE),
                col("intellect", 0x05, LOW_NIBBLE),
                col("ability", 0x06, Ability),
                col("extra ability", 0x07, Ability),
            ],
        ),
        party_sheet(),
        encounter_sheet(),
        enemy_sheet(),
        enemy_ai_sheet(),
        actor_def_sheet(),
        cue_sheet(),
        vab_bank_sheet(),
        vab_program_sheet(),
        vab_tone_sheet(),
    ]
}

const SHOP_SLOT_CATEGORY: [&str; tables::SHOP_SLOTS] = [
    "1 category",
    "2 category",
    "3 category",
    "4 category",
    "5 category",
    "6 category",
    "7 category",
    "8 category",
    "9 category",
    "10 category",
    "11 category",
];
const SHOP_SLOT_ITEM: [&str; tables::SHOP_SLOTS] =
    ["1 item", "2 item", "3 item", "4 item", "5 item", "6 item", "7 item", "8 item", "9 item", "10 item", "11 item"];

/// `ShopStock`: an item count, then eleven `ShopItemRef` slots of category and index.
fn shop_columns() -> Vec<Column> {
    let mut columns = vec![col("item count", 0, Field::U8)];
    for slot in 0..tables::SHOP_SLOTS {
        let category = 1 + 2 * slot;
        columns.push(col(SHOP_SLOT_CATEGORY[slot], category, Field::Choice(ITEM_CATEGORIES)));
        columns.push(col(SHOP_SLOT_ITEM[slot], category + 1, Field::ItemAt(category)));
    }
    columns
}

/// The payload offset of the first party record in `ETC/START.EMI` entry 8.
const PARTY_START: usize = rebof3_formats::characters::PAYLOAD_OFFSET;
/// `Character`: the record offsets of its stat blocks.
const PARTY_STATS: usize = 0x1c;
const PARTY_BASE: usize = 0x3c;

/// `Character` (`include/game/character.h`): the party members' starting records.
fn party_sheet() -> Table {
    use Field::*;
    let mut columns = vec![
        col("name", 0x00, Name(rebof3_formats::characters::NAME_BYTES)),
        col("character id", 0x05, U8),
        col("level", 0x06, U8),
        col("exp", 0x08, U32),
        col("status", 0x0c, Bits(STATUS)),
        col("weapon", 0x0e, ItemOf(CATEGORY_WEAPON)),
        col("shield", 0x0f, ItemOf(CATEGORY_ARMOR)),
        col("helmet", 0x10, ItemOf(CATEGORY_ARMOR)),
        col("armor", 0x11, ItemOf(CATEGORY_ARMOR)),
        col("accessory 1", 0x12, ItemOf(CATEGORY_ACCESSORY)),
        col("accessory 2", 0x13, ItemOf(CATEGORY_ACCESSORY)),
        col("hp", 0x14, U16),
        col("ap", 0x16, U16),
        col("willpower", 0x18, U8),
        col("inoculations", 0x19, U8),
        col("fatigue", 0x1a, U8),
        col("master", 0x1b, Choice(MASTERS)),
    ];
    columns.extend(stat_columns(STAT_NAMES, PARTY_STATS));
    columns.extend(stat_columns(BASE_STAT_NAMES, PARTY_BASE));
    columns.push(col("master level", 0x84, U8));

    Table {
        name: "party",
        place: Place::Entry { archive: START, entry: START_PARTY_ENTRY },
        start: PARTY_START,
        count: Count::Fixed(rebof3_formats::characters::CHARACTERS),
        record: rebof3_formats::characters::RECORD_BYTES,
        columns,
    }
}

/// The six `u16` stats that open a `Stats` block: the live copy, then the base the equipment adds to.
const STAT_NAMES: [&str; 6] = ["max hp", "max ap", "power", "defense", "agility", "intellect"];
const BASE_STAT_NAMES: [&str; 6] =
    ["base max hp", "base max ap", "base power", "base defense", "base agility", "base intellect"];

/// The columns of the six `u16` stats of the `Stats` block at record offset `block`.
fn stat_columns(names: [&'static str; 6], block: usize) -> Vec<Column> {
    names.into_iter().enumerate().map(|(at, name)| col(name, block + 2 * at, Field::U16)).collect()
}

const ENCOUNTER_SLOTS: [&str; ENCOUNTER_ENEMIES] =
    ["enemy 1", "enemy 2", "enemy 3", "enemy 4", "enemy 5", "enemy 6", "enemy 7", "enemy 8"];

/// `Encounter`: eight enemy indexes and an appearance rate.
fn encounter_sheet() -> Table {
    let mut columns: Vec<Column> =
        ENCOUNTER_SLOTS.into_iter().enumerate().map(|(at, name)| col(name, at, Field::Enemy)).collect();
    columns.push(col("appearance rate", ENCOUNTER_ENEMIES, Field::U8));
    Table {
        name: "encounters",
        place: Place::Format(Format::AreaEncounterTable),
        start: 0,
        count: Count::Fixed(ENCOUNTERS),
        record: ENCOUNTER_BYTES,
        columns,
    }
}

/// The record offsets of an enemy's two drop rows (`EnemyDrop`); Steal takes the first.
const ENEMY_DROPS: [usize; 2] = [0x2c, 0x30];
/// The record offset of an enemy's resistance levels.
const ENEMY_RESISTANCES: usize = 0x78;
/// `ResistanceIndex`.
const RESISTANCES: [&str; 9] = ["fire", "frost", "electric", "earth", "wind", "holy", "psionic", "status", "death"];
const INITIAL_SKILLS: [&str; 8] =
    ["skill 1", "skill 2", "skill 3", "skill 4", "skill 5", "skill 6", "skill 7", "skill 8"];

/// `Enemy` (`include/game/enemy.h`): the eight enemy records after the encounters.
fn enemy_sheet() -> Table {
    use Field::*;
    let mut columns = vec![
        col("name", 0x00, Name(encounter::NAME_BYTES)),
        col("enemy id", 0x08, U8),
        col("family", 0x09, Choice(ENEMY_FAMILIES)),
        col("choice AI", 0x0a, U8),
        col("target preference", 0x0e, U8),
        col("zenny", 0x10, U16),
        col("exp", 0x12, U16),
        col("level", 0x14, U16),
        col("hp", 0x20, U16),
        col("ap", 0x22, U16),
        col("power", 0x24, U16),
        col("defense", 0x26, U16),
        col("agility", 0x28, U16),
        col("intellect", 0x2a, U16),
    ];
    const DROP_NAMES: [[&str; 3]; 2] =
        [["drop 1 item", "drop 1 category", "drop 1 rate"], ["drop 2 item", "drop 2 category", "drop 2 rate"]];
    for (row, [item, category, rate]) in ENEMY_DROPS.into_iter().zip(DROP_NAMES) {
        columns.push(col(item, row, ItemAt(row + 1)));
        columns.push(col(category, row + 1, Choice(ITEM_CATEGORIES)));
        columns.push(col(rate, row + 2, U8));
    }
    for (at, name) in INITIAL_SKILLS.into_iter().enumerate() {
        columns.push(col(name, 0x18 + at, Ability));
    }
    for (at, name) in RESISTANCES.into_iter().enumerate() {
        columns.push(col(name, ENEMY_RESISTANCES + at, U8));
    }
    columns.push(col("status", 0x81, Bits(STATUS)));
    columns.push(col("reach", 0x82, U8));
    columns.push(col("height", 0x83, U8));

    Table {
        name: "enemies",
        place: Place::Format(Format::AreaEncounterTable),
        start: ENEMY_BASE,
        count: Count::Fixed(ENEMIES),
        record: ENEMY_BYTES,
        columns,
    }
}

/// The record offset of an enemy's four `EnemyAIBlock`s, and the bytes of one.
const ENEMY_AI: usize = 0x34;
const AI_BLOCK_BYTES: usize = 0x10;

/// The columns of `EnemyAIBlock` number `$block`: a condition, seven parameters and eight skills.
macro_rules! ai_block {
    ($block:literal) => {{
        let at = ENEMY_AI + ($block - 1) * AI_BLOCK_BYTES;
        [
            col(concat!("AI ", $block, " condition"), at, Field::U8),
            col(concat!("AI ", $block, " parameter 1"), at + 1, Field::U8),
            col(concat!("AI ", $block, " parameter 2"), at + 2, Field::U8),
            col(concat!("AI ", $block, " parameter 3"), at + 3, Field::U8),
            col(concat!("AI ", $block, " parameter 4"), at + 4, Field::U8),
            col(concat!("AI ", $block, " parameter 5"), at + 5, Field::U8),
            col(concat!("AI ", $block, " parameter 6"), at + 6, Field::U8),
            col(concat!("AI ", $block, " parameter 7"), at + 7, Field::U8),
            col(concat!("AI ", $block, " skill 1"), at + 8, Field::Ability),
            col(concat!("AI ", $block, " skill 2"), at + 9, Field::Ability),
            col(concat!("AI ", $block, " skill 3"), at + 10, Field::Ability),
            col(concat!("AI ", $block, " skill 4"), at + 11, Field::Ability),
            col(concat!("AI ", $block, " skill 5"), at + 12, Field::Ability),
            col(concat!("AI ", $block, " skill 6"), at + 13, Field::Ability),
            col(concat!("AI ", $block, " skill 7"), at + 14, Field::Ability),
            col(concat!("AI ", $block, " skill 8"), at + 15, Field::Ability),
        ]
    }};
}

/// The four `EnemyAIBlock`s of each enemy record. A skill byte of 99 marks an unused block.
fn enemy_ai_sheet() -> Table {
    let mut columns = vec![col("name", 0x00, Field::Name(encounter::NAME_BYTES))];
    columns.extend(ai_block!(1));
    columns.extend(ai_block!(2));
    columns.extend(ai_block!(3));
    columns.extend(ai_block!(4));
    Table {
        name: "enemy AI",
        place: Place::Format(Format::AreaEncounterTable),
        start: ENEMY_BASE,
        count: Count::Fixed(ENEMIES),
        record: ENEMY_BYTES,
        columns,
    }
}

/// `SpriteTextureMode`, the texture mode of an actor definition.
const SPRITE_TEXTURE_MODES: &[(u8, &str)] = &[(0, "0"), (1, "1"), (2, "2"), (3, "3"), (4, "4"), (9, "9"), (10, "10")];

/// `AreaActorDefs` (`include/world/area.h`): a `u32` count, then eight-byte `AreaActorDef` records.
fn actor_def_sheet() -> Table {
    use Field::*;
    Table {
        name: "actor definitions",
        place: Place::Format(Format::AreaActorDefsTable),
        start: rebof3_formats::scene::COUNT_BYTES,
        count: Count::Word(0),
        record: rebof3_formats::scene::RECORD_BYTES,
        columns: vec![
            col("id", 0, U16),
            col("clut select", 2, U16),
            col("texture page", 4, U8),
            col("texture v", 5, U8),
            col("texture mode", 6, Choice(SPRITE_TEXTURE_MODES)),
            col("script section", 7, U8),
        ],
    }
}

/// A type-8 cue record (`EMICue`, `include/audio/cue.h`): the SPU voice, VAB program and tone of one
/// sound effect, packed in four bytes.
fn cue_sheet() -> Table {
    use Field::*;
    Table {
        name: "cues",
        place: Place::Format(Format::CueTable),
        start: 0,
        count: Count::Payload,
        record: rebof3_emi::cue::RECORD_BYTES,
        columns: vec![
            col("alias bank", 0, Part { shift: 0, bits: 3 }),
            col("program", 1, Part { shift: 0, bits: 7 }),
            col("cue volume", 1, Part { shift: 7, bits: 1 }),
            col("group", 2, Part { shift: 0, bits: 4 }),
            col("tone", 2, Part { shift: 4, bits: 4 }),
            col("voice", 3, Part { shift: 0, bits: 5 }),
            col("layers", 3, Part { shift: 5, bits: 2 }),
        ],
    }
}

/// VAB bank geometry (`include/audio/vab.h`): the `VabHdr` header, the fixed program table and the
/// tone table, whose blocks the header's program count gives.
const VAB_HEADER_SIZE: usize = 0x20;
const VAB_PROGRAM_TABLE: usize = 0x20;
const VAB_PROGRAM_SIZE: usize = 0x10;
const VAB_PROGRAM_MAX: usize = 128;
const VAB_TONE_TABLE: usize = 0x820;
const VAB_TONE_SIZE: usize = 0x20;
const VAB_TONES_PER_PROGRAM: usize = 16;
/// `VabHdr.ps`, the header's program count, which sizes the tone table.
const VAB_PROGRAM_COUNT: usize = 0x12;

/// `VabHdr` (PsyQ `libsnd.h`): the bank's master volume, pan and attributes. The counts and sizes
/// that lay out the header are left out, so an edit cannot move a table.
fn vab_bank_sheet() -> Table {
    use Field::*;
    Table {
        name: "bank",
        place: Place::Format(Format::VabHeader),
        start: 0,
        count: Count::Fixed(1),
        record: VAB_HEADER_SIZE,
        columns: vec![
            col("id", 0x08, U32),
            col("master volume", 0x18, U8),
            col("master pan", 0x19, U8),
            col("attribute 1", 0x1a, U8),
            col("attribute 2", 0x1b, U8),
        ],
    }
}

/// `ProgAtr` (PsyQ `libsnd.h`): one record per program slot; slot 0 is the bank's dummy program.
fn vab_program_sheet() -> Table {
    use Field::*;
    Table {
        name: "programs",
        place: Place::Format(Format::VabHeader),
        start: VAB_PROGRAM_TABLE,
        count: Count::Fixed(VAB_PROGRAM_MAX),
        record: VAB_PROGRAM_SIZE,
        columns: vec![
            col("tones", 0x00, U8),
            col("volume", 0x01, U8),
            col("priority", 0x02, U8),
            col("mode", 0x03, U8),
            col("pan", 0x04, U8),
            col("attribute", 0x06, U16),
        ],
    }
}

/// `VagAtr` (PsyQ `libsnd.h`, `include/audio/vab.h`): sixteen tone slots per program block.
fn vab_tone_sheet() -> Table {
    use Field::*;
    Table {
        name: "tones",
        place: Place::Format(Format::VabHeader),
        start: VAB_TONE_TABLE,
        count: Count::Blocks { at: VAB_PROGRAM_COUNT, per: VAB_TONES_PER_PROGRAM },
        record: VAB_TONE_SIZE,
        columns: vec![
            col("priority", 0x00, U8),
            col("mode", 0x01, U8),
            col("volume", 0x02, U8),
            col("pan", 0x03, U8),
            col("center note", 0x04, U8),
            col("pitch tune", 0x05, U8),
            col("min note", 0x06, U8),
            col("max note", 0x07, U8),
            col("vibrato width", 0x08, U8),
            col("vibrato time", 0x09, U8),
            col("portamento width", 0x0a, U8),
            col("portamento time", 0x0b, U8),
            col("bend down", 0x0c, U8),
            col("bend up", 0x0d, U8),
            col("ADSR1", 0x10, U16),
            col("ADSR2", 0x12, U16),
            col("program", 0x14, U16),
            col("VAG", 0x16, U16),
        ],
    }
}

/// The names a reference column shows: the item and ability names of `GAME.EMI` entry 0, and the
/// enemy names of the shown encounter block.
#[derive(Default)]
pub struct Names {
    items: [Vec<String>; CATEGORY_TABLES.len()],
    abilities: Vec<String>,
    enemies: Vec<String>,
}

impl Names {
    /// The names in `game`, the payload of `GAME.EMI` entry 0, and in `shown`, the shown entry's payload
    /// when it is an encounter block.
    pub fn new(game: Option<&[u8]>, shown: Option<&[u8]>) -> Self {
        let names = |table: tables::Table| -> Vec<String> {
            game.and_then(|payload| table.records(payload).ok())
                .map(|records| records.iter().map(|record| tables::name_text(&record[..tables::NAME_BYTES])).collect())
                .unwrap_or_default()
        };
        let enemies = shown
            .filter(|payload| payload.len() >= encounter::BLOCK_BYTES)
            .map(|payload| {
                (0..ENEMIES)
                    .map(|index| {
                        let start = ENEMY_BASE + index * ENEMY_BYTES;
                        tables::name_text(&payload[start..start + encounter::NAME_BYTES])
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self { items: CATEGORY_TABLES.map(names), abilities: names(tables::ABILITIES), enemies }
    }

    /// The name of item `index` in `category`.
    pub fn item(&self, category: u8, index: u8) -> Option<&str> {
        self.items.get(usize::from(category))?.get(usize::from(index)).map(String::as_str)
    }

    pub fn ability(&self, index: u8) -> Option<&str> {
        self.abilities.get(usize::from(index)).map(String::as_str)
    }

    pub fn enemy(&self, index: u8) -> Option<&str> {
        self.enemies.get(usize::from(index)).map(String::as_str)
    }

    /// The name a reference column shows for `record`, or `None` for a column that names nothing.
    pub fn of(&self, record: &[u8], column: &Column) -> Option<&str> {
        let value = record[column.offset];
        match column.field {
            Field::ItemAt(category) => self.item(record[category], value),
            Field::ItemOf(category) => self.item(category, value),
            Field::Ability => self.ability(value),
            Field::Enemy if value != EMPTY_ENEMY => self.enemy(value),
            _ => None,
        }
    }
}

/// The text of a name field.
pub fn name_text(record: &[u8], column: &Column) -> String {
    let Field::Name(width) = column.field else { return String::new() };
    tables::name_text(&record[column.offset..column.offset + width])
}

/// The bytes of a name field, or why the text does not fit.
pub fn name_bytes(text: &str, column: &Column) -> Result<Vec<u8>, String> {
    let Field::Name(width) = column.field else { return Err("not a name".into()) };
    tables::name_field(text, width).map_err(|error| error.to_string())
}

/// The value a column reads.
pub fn read(record: &[u8], column: &Column) -> u32 {
    let at = column.offset;
    match column.field {
        Field::U16 => u32::from(u16::from_le_bytes([record[at], record[at + 1]])),
        Field::U32 => u32::from_le_bytes([record[at], record[at + 1], record[at + 2], record[at + 3]]),
        Field::Part { shift, bits } => u32::from(record[at] >> shift) & mask(bits),
        Field::Name(_) => 0,
        Field::U8
        | Field::Bits(_)
        | Field::Choice(_)
        | Field::ItemAt(_)
        | Field::ItemOf(_)
        | Field::Ability
        | Field::Enemy => u32::from(record[at]),
    }
}

/// Writes a column's value, clamped to the field's range.
pub fn write(record: &mut [u8], column: &Column, value: u32) {
    let at = column.offset;
    let value = value.min(max(column));
    match column.field {
        Field::U16 => record[at..at + 2].copy_from_slice(&(value as u16).to_le_bytes()),
        Field::U32 => record[at..at + 4].copy_from_slice(&value.to_le_bytes()),
        Field::Part { shift, bits } => {
            let mask = (mask(bits) as u8) << shift;
            record[at] = (record[at] & !mask) | ((value as u8) << shift & mask);
        }
        Field::Name(_) => {}
        Field::U8
        | Field::Bits(_)
        | Field::Choice(_)
        | Field::ItemAt(_)
        | Field::ItemOf(_)
        | Field::Ability
        | Field::Enemy => record[at] = value as u8,
    }
}

/// The largest value `bits` bits hold.
fn mask(bits: u8) -> u32 {
    (1 << bits) - 1
}

/// The maximum value of a numeric column.
pub fn max(column: &Column) -> u32 {
    match column.field {
        Field::U16 => 0xFFFF,
        Field::U32 => u32::MAX,
        Field::Part { bits, .. } => mask(bits),
        _ => 0xFF,
    }
}

/// The bytes a column covers in its record.
pub fn width(column: &Column) -> usize {
    match column.field {
        Field::Name(width) => width,
        Field::U16 => 2,
        Field::U32 => 4,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A record of distinct bytes, so a write to the wrong offset shows.
    fn record(bytes: usize) -> Vec<u8> {
        (0..bytes).map(|at| (at * 7 + 3) as u8).collect()
    }

    #[test]
    fn every_column_fits_its_record() {
        for sheet in sheets() {
            for column in &sheet.columns {
                let end = column.offset + width(column);
                assert!(end <= sheet.record, "{}: {} ends at {end:#x}", sheet.name, column.name);
                if let Field::ItemAt(category) = column.field {
                    assert!(category < sheet.record, "{}: {}", sheet.name, column.name);
                }
            }
        }
    }

    #[test]
    fn writing_every_value_back_keeps_every_byte() {
        for sheet in sheets() {
            let stored = record(sheet.record);
            let mut copy = stored.clone();
            for column in &sheet.columns {
                if let Field::Name(_) = column.field {
                    let bytes = name_bytes(&name_text(&copy, column), column);
                    if let Ok(bytes) = bytes {
                        copy[column.offset..column.offset + bytes.len()].copy_from_slice(&bytes);
                    }
                    continue;
                }
                let value = read(&copy, column);
                write(&mut copy, column, value);
            }
            assert_eq!(copy, stored, "{}", sheet.name);
        }
    }

    #[test]
    fn a_write_changes_only_its_column_bytes() {
        for sheet in sheets() {
            let stored = record(sheet.record);
            for column in sheet.columns.iter().filter(|column| !matches!(column.field, Field::Name(_))) {
                let mut copy = stored.clone();
                write(&mut copy, column, read(&stored, column) ^ 1);
                let span = column.offset..column.offset + width(column);
                for at in (0..sheet.record).filter(|at| !span.contains(at)) {
                    assert_eq!(copy[at], stored[at], "{}: {} wrote byte {at:#x}", sheet.name, column.name);
                }
                assert_ne!(copy, stored, "{}: {}", sheet.name, column.name);
            }
        }
    }

    #[test]
    fn a_part_write_keeps_the_other_bits_of_its_byte() {
        let program = col("program", 0, Field::Part { shift: 0, bits: 7 });
        let volume = col("cue volume", 0, Field::Part { shift: 7, bits: 1 });
        let mut record = [0x85];
        assert_eq!((read(&record, &program), read(&record, &volume)), (5, 1));
        write(&mut record, &program, 0x7F);
        assert_eq!(record, [0xFF]);
        write(&mut record, &volume, 0);
        assert_eq!(record, [0x7F]);
        assert_eq!(max(&program), 0x7F);
    }

    #[test]
    fn a_counted_sheet_reads_its_count_word() {
        let sheet = actor_def_sheet();
        let mut payload = 2u32.to_le_bytes().to_vec();
        payload.extend([0; 16]);
        assert_eq!(sheet.rows(&payload), 2);
        assert_eq!(sheet.end(&payload), payload.len());
        assert_eq!(cue_sheet().rows(&[0; 10]), 2, "a partial record is not a row");
    }

    #[test]
    fn the_tone_sheet_has_sixteen_slots_per_program_block() {
        let tones = vab_tone_sheet();
        let mut header = vec![0; VAB_TONE_TABLE];
        header[VAB_PROGRAM_COUNT..VAB_PROGRAM_COUNT + 2].copy_from_slice(&3u16.to_le_bytes());
        assert_eq!(tones.rows(&header), 3 * VAB_TONES_PER_PROGRAM);
        assert_eq!(tones.end(&header), VAB_TONE_TABLE + 3 * VAB_TONES_PER_PROGRAM * VAB_TONE_SIZE);
        assert_eq!(tones.rows(&header[..VAB_PROGRAM_COUNT]), 0, "a header without the count has no tones");
        assert_eq!(vab_program_sheet().end(&header), VAB_TONE_TABLE);
    }

    #[test]
    fn a_nibble_write_keeps_the_other_nibble() {
        let high = col("power", 0, HIGH_NIBBLE);
        let low = col("defense", 0, LOW_NIBBLE);
        let mut record = [0xA5];
        write(&mut record, &high, 0x3);
        assert_eq!(record, [0x35]);
        write(&mut record, &low, 0x1F);
        assert_eq!(record, [0x3F], "a value past the nibble clamps to 15");
    }

    #[test]
    fn encounter_sheets_apply_to_every_encounter_block_and_game_sheets_to_one_entry() {
        let area = sheets_for("BIN/WORLD00/AREA001.EMI", 3, Format::AreaEncounterTable);
        assert_eq!(area.iter().map(|sheet| sheet.name).collect::<Vec<_>>(), ["encounters", "enemies", "enemy AI"]);
        assert_eq!(sheets_for(GAME, 0, Format::CodeImage).len(), 8);
        assert!(sheets_for(GAME, 1, Format::CodeImage).is_empty());
        assert_eq!(sheets_for(START, START_PARTY_ENTRY, Format::Unclassified).len(), 1);
        let vab = sheets_for("BIN/SND/BGM000.EMI", 0, Format::VabHeader);
        assert_eq!(vab.iter().map(|sheet| sheet.name).collect::<Vec<_>>(), ["bank", "programs", "tones"]);
    }

    #[test]
    fn the_enemy_sheet_ends_at_the_block_end() {
        let enemies = enemy_sheet();
        assert_eq!(enemies.end(&[]), encounter::BLOCK_BYTES);
        assert_eq!(encounter_sheet().end(&[]), ENEMY_BASE);
    }

    /// A game payload long enough for every table, with one weapon and one ability named.
    fn game_payload() -> Vec<u8> {
        let end = tables::TABLES
            .iter()
            .map(|table| table.archive_offset - tables::PAYLOAD_START + table.count * table.record);
        let mut payload = vec![0; end.max().unwrap()];
        let mut put = |table: tables::Table, index: usize, name: &str| {
            let at = table.archive_offset - tables::PAYLOAD_START + index * table.record;
            let bytes = tables::name_field(name, tables::NAME_BYTES).unwrap();
            payload[at..at + bytes.len()].copy_from_slice(&bytes);
        };
        put(tables::WEAPONS, 3, "Sword");
        put(tables::ABILITIES, 2, "Flame");
        payload
    }

    #[test]
    fn reference_columns_name_the_record_they_point_at() {
        let mut block = vec![EMPTY_ENEMY; encounter::BLOCK_BYTES];
        let worker = tables::name_field("Worker", encounter::NAME_BYTES).unwrap();
        block[ENEMY_BASE + ENEMY_BYTES..ENEMY_BASE + ENEMY_BYTES + worker.len()].copy_from_slice(&worker);
        let names = Names::new(Some(&game_payload()), Some(&block));

        let drop = col("drop 1 item", 0, Field::ItemAt(1));
        assert_eq!(names.of(&[3, CATEGORY_WEAPON], &drop), Some("Sword"));
        assert_eq!(names.of(&[3, EMPTY_CATEGORY], &drop), None);
        assert_eq!(names.of(&[2], &col("skill 1", 0, Field::Ability)), Some("Flame"));
        assert_eq!(names.of(&[1], &col("enemy 1", 0, Field::Enemy)), Some("Worker"));
        assert_eq!(names.of(&[EMPTY_ENEMY], &col("enemy 1", 0, Field::Enemy)), None);
        assert_eq!(names.item(CATEGORY_ITEM, 200), None, "past the item table");
    }
}
