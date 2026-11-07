//! Record layouts the editor shows as spreadsheet columns.
//!
//! Each column reads and writes the record bytes directly, at the offsets `rebof3_emi::game_tables`
//! documents, so a column edit changes exactly the bytes it names.

use rebof3_emi::game_tables::{self as tables, NAME_BYTES, Table};

#[derive(Clone, Copy)]
pub enum Field {
    /// An inline name in the name encoding.
    Name,
    U8,
    U16,
    /// The high (`true`) or low nibble of a byte.
    Nibble(bool),
    /// A byte of flag bits, bit 0 first.
    Bits(&'static [&'static str]),
    /// A byte that picks one of a few values.
    Choice(&'static [(u8, &'static str)]),
}

#[derive(Clone, Copy)]
pub struct Column {
    pub name: &'static str,
    pub offset: usize,
    pub field: Field,
}

const fn col(name: &'static str, offset: usize, field: Field) -> Column {
    Column { name, offset, field }
}

const ITEM_FLAGS: &[&str] =
    &["target both", "selectable", "enemy default", "target all", "story", "show name", "show anim", "menu use"];
const ELEMENTS: &[&str] = &["fire", "ice", "lightning", "earth", "wind", "holy", "psionic", "status"];
const ABILITY_BITS: &[&str] =
    &["bit 0", "examinable", "ally default", "bit 3", "affects stats", "bit 5", "bit 6", "bit 7"];
const PARTY: &[&str] = &["Ryu", "Nina", "Garr", "Teepo", "Rei", "Momo", "Peco", "Whelp"];
const EQUIP_TYPES: &[(u8, &str)] = &[(0, "nothing"), (2, "shield"), (3, "helmet"), (4, "body")];
const ITEM_TYPES: &[(u8, &str)] =
    &[(0, "item"), (1, "weapon"), (2, "armor"), (3, "accessory"), (4, "vital"), (0xFF, "empty")];
const ABILITY_TYPES: &[(u8, &str)] = &[(0, "heal"), (1, "assist"), (2, "attack"), (3, "skill")];

/// A table, the archive and entry that hold it, and its columns. Field offsets follow
/// `wiki: data/records#row-layouts`.
pub struct Sheet {
    pub archive: &'static str,
    pub entry: usize,
    pub table: Table,
    pub columns: Vec<Column>,
}

impl Sheet {
    /// The payload offset of the first record.
    pub fn start(&self) -> usize {
        self.table.archive_offset - tables::PAYLOAD_START
    }
}

const GAME: &str = "BIN/ETC/GAME.EMI";

fn sheet(table: Table, columns: Vec<Column>) -> Sheet {
    Sheet { archive: GAME, entry: 0, table, columns }
}

pub fn sheets() -> Vec<Sheet> {
    use Field::*;
    let mut shop = vec![col("count", 0, U8)];
    for slot in 0..tables::SHOP_SLOTS {
        shop.push(col(SLOT_TYPE[slot], 1 + 2 * slot, Choice(ITEM_TYPES)));
        shop.push(col(SLOT_INDEX[slot], 2 + 2 * slot, U8));
    }
    vec![
        sheet(
            tables::ITEMS,
            vec![
                col("name", 0, Name),
                col("flags", 0x0c, Bits(ITEM_FLAGS)),
                col("unk 0D", 0x0d, U8),
                col("description", 0x0e, U16),
                col("price", 0x10, U16),
            ],
        ),
        sheet(tables::VITAL_ITEMS, vec![col("name", 0, Name), col("description", 0x0c, U16), col("unk 0E", 0x0e, U16)]),
        sheet(
            tables::WEAPONS,
            vec![
                col("name", 0, Name),
                col("equip", 0x0c, Bits(PARTY)),
                col("flags", 0x0d, U8),
                col("kind", 0x0e, U8),
                col("element", 0x0f, Bits(ELEMENTS)),
                col("weight", 0x10, U8),
                col("sound", 0x11, U8),
                col("power", 0x12, U8),
                col("unk 13", 0x13, U8),
                col("description", 0x14, U16),
                col("price", 0x16, U16),
            ],
        ),
        sheet(
            tables::ARMOR,
            vec![
                col("name", 0, Name),
                col("equip", 0x0c, Bits(PARTY)),
                col("flags", 0x0d, U8),
                col("type", 0x0e, Choice(EQUIP_TYPES)),
                col("weight", 0x0f, U8),
                col("power", 0x10, U8),
                col("unk 11", 0x11, U8),
                col("description", 0x12, U16),
                col("price", 0x14, U16),
            ],
        ),
        sheet(
            tables::ACCESSORIES,
            vec![
                col("name", 0, Name),
                col("equip", 0x0c, Bits(PARTY)),
                col("flags", 0x0d, U8),
                col("kind", 0x0e, U8),
                col("weight", 0x0f, U8),
                col("description", 0x10, U16),
                col("price", 0x12, U16),
            ],
        ),
        sheet(tables::SHOPS, shop),
        sheet(
            tables::ABILITIES,
            vec![
                col("name", 0, Name),
                col("targeting", 0x0c, U8),
                col("type", 0x0d, Choice(ABILITY_TYPES)),
                col("cost", 0x0e, U8),
                col("power", 0x0f, U8),
                col("element", 0x10, Bits(ELEMENTS)),
                col("flags", 0x11, Bits(ABILITY_BITS)),
                col("description", 0x12, U16),
            ],
        ),
        sheet(
            tables::LEVEL_GROWTH,
            vec![
                col("exp", 0, U16),
                col("hp", 2, U8),
                col("ap", 3, U8),
                col("power", 4, Nibble(true)),
                col("defense", 4, Nibble(false)),
                col("agility", 5, Nibble(true)),
                col("intellect", 5, Nibble(false)),
                col("ability", 6, U8),
                col("extra ability", 7, U8),
            ],
        ),
    ]
}

const SLOT_TYPE: [&str; tables::SHOP_SLOTS] =
    ["type 1", "type 2", "type 3", "type 4", "type 5", "type 6", "type 7", "type 8", "type 9", "type 10", "type 11"];
const SLOT_INDEX: [&str; tables::SHOP_SLOTS] = [
    "index 1", "index 2", "index 3", "index 4", "index 5", "index 6", "index 7", "index 8", "index 9", "index 10",
    "index 11",
];

/// The text of a name field.
pub fn name_text(record: &[u8], offset: usize) -> String {
    tables::name_text(&record[offset..offset + NAME_BYTES])
}

/// The bytes of a name field, or why the text does not fit.
pub fn name_bytes(text: &str) -> Result<Vec<u8>, String> {
    tables::name_field(text, NAME_BYTES).map_err(|error| error.to_string())
}

/// The value a column reads.
pub fn read(record: &[u8], column: &Column) -> u32 {
    let at = column.offset;
    match column.field {
        Field::U16 => u32::from(u16::from_le_bytes([record[at], record[at + 1]])),
        Field::Nibble(true) => u32::from(record[at] >> 4),
        Field::Nibble(false) => u32::from(record[at] & 0x0F),
        Field::Name => 0,
        Field::U8 | Field::Bits(_) | Field::Choice(_) => u32::from(record[at]),
    }
}

/// Writes a column's value, clamped to the field's range.
pub fn write(record: &mut [u8], column: &Column, value: u32) {
    let at = column.offset;
    match column.field {
        Field::U16 => record[at..at + 2].copy_from_slice(&(value.min(0xFFFF) as u16).to_le_bytes()),
        Field::Nibble(high) => {
            let nibble = value.min(0x0F) as u8;
            record[at] = if high { (record[at] & 0x0F) | (nibble << 4) } else { (record[at] & 0xF0) | nibble };
        }
        Field::Name => {}
        Field::U8 | Field::Bits(_) | Field::Choice(_) => record[at] = value.min(0xFF) as u8,
    }
}

/// The maximum value of a numeric column.
pub fn max(column: &Column) -> u32 {
    match column.field {
        Field::U16 => 0xFFFF,
        Field::Nibble(_) => 0x0F,
        _ => 0xFF,
    }
}
