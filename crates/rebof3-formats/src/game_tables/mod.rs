//! The fixed data tables of `ETC/GAME.EMI` entry 0: items, vital items, weapons, armor, accessories,
//! shops, abilities and level growth.
//!
//! The entry's payload starts at archive offset [`PAYLOAD_START`]. Each table is a run of fixed-size
//! records at a fixed archive offset (`wiki: data/records#gameemi-fixed-tables`, "Equipment and shop
//! data"). The runtime
//! item base `0x801C8964` is the entry's load address `0x80195800` plus the items' payload offset.
//!
//! A name is 12 inline bytes in the name encoding ([`rebof3_emi::text::codec`]). Its text leaves out the zeros that pad
//! the field, so the stored bytes come back exactly. The other fields keep the documented offsets. Fields the
//! decompilation has not named are left out.

use rebof3_emi::record::{bytes, u16_at};
pub use rebof3_emi::text::codec::{name_field, name_text};

use crate::error::{Error, Result};

pub mod open;

/// Archive offset where the entry's payload starts.
pub const PAYLOAD_START: usize = 0x800;
/// Bytes in an inline name.
pub const NAME_BYTES: usize = 12;
/// Item slots in a shop record.
pub const SHOP_SLOTS: usize = 11;

/// One fixed table: its name, the archive offset of its first record, its record count and its size.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Table {
    /// The table's name in the catalog and in its JSON file.
    pub name: &'static str,
    /// The archive offset of the first record.
    pub archive_offset: usize,
    /// The number of records.
    pub count: usize,
    /// The size of one record in bytes.
    pub record: usize,
}

pub const ITEMS: Table = Table { name: "items", archive_offset: 0x33964, count: 92, record: 0x12 };
pub const VITAL_ITEMS: Table = Table { name: "vital-items", archive_offset: 0x33fdc, count: 16, record: 0x10 };
pub const WEAPONS: Table = Table { name: "weapons", archive_offset: 0x340dc, count: 83, record: 0x18 };
pub const ARMOR: Table = Table { name: "armor", archive_offset: 0x348a4, count: 68, record: 0x16 };
pub const ACCESSORIES: Table = Table { name: "accessories", archive_offset: 0x34e7c, count: 52, record: 0x14 };
pub const SHOPS: Table = Table { name: "shops", archive_offset: 0x3528c, count: 40, record: 0x17 };
pub const ABILITIES: Table = Table { name: "abilities", archive_offset: 0x3570c, count: 228, record: 0x14 };
pub const LEVEL_GROWTH: Table = Table { name: "level-growth", archive_offset: 0x368dc, count: 693, record: 0x08 };

/// Every table, in the order the catalog lists them.
pub const TABLES: [Table; 8] = [ITEMS, VITAL_ITEMS, WEAPONS, ARMOR, ACCESSORIES, SHOPS, ABILITIES, LEVEL_GROWTH];

impl Table {
    /// The table's records in an entry payload, one slice per record. Fails when the payload ends before
    /// the last record.
    pub fn records(self, payload: &[u8]) -> Result<Vec<&[u8]>> {
        Ok(self.bytes(payload)?.chunks_exact(self.record).collect())
    }

    /// The table's stored bytes in an entry payload: all its records, back to back. Fails when the payload ends
    /// before the last record.
    pub fn bytes(self, payload: &[u8]) -> Result<&[u8]> {
        let start = self.archive_offset - PAYLOAD_START;
        let end = start + self.count * self.record;
        payload.get(start..end).ok_or(Error::GameTableShort { table: self.name, bytes: payload.len(), needed: end })
    }
}

fn serialize_name<S: serde::Serializer>(name: &[u8; NAME_BYTES], serializer: S) -> std::result::Result<S::Ok, S::Error> {
    serializer.serialize_str(&name_text(name))
}

fn deserialize_name<'de, D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<[u8; NAME_BYTES], D::Error> {
    let text = <String as serde::Deserialize>::deserialize(deserializer)?;
    let bytes = name_field(&text, NAME_BYTES).map_err(<D::Error as serde::de::Error>::custom)?;
    Ok(bytes.try_into().expect("a name field is NAME_BYTES long"))
}

/// One item: a consumable or a vital item of the item table.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct Item {
    #[serde(serialize_with = "serialize_name", deserialize_with = "deserialize_name")]
    pub name: [u8; NAME_BYTES],
    /// Flag bits: `target_both`, `target_selectable`, `target_enemy_default`, `target_all`, `story_item`,
    /// `show_name`, `show_animation`, `usable_menu`.
    pub flags: u8,
    pub price: u16,
}

/// One vital item. Vital items are story-critical and cannot be sold.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct VitalItem {
    #[serde(serialize_with = "serialize_name", deserialize_with = "deserialize_name")]
    pub name: [u8; NAME_BYTES],
}

/// One weapon. `equipability` marks the characters that can equip it.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct Weapon {
    #[serde(serialize_with = "serialize_name", deserialize_with = "deserialize_name")]
    pub name: [u8; NAME_BYTES],
    pub equipability: u8,
    pub element: u8,
    pub weight: u8,
    pub power: u8,
    pub price: u16,
}

/// One armor piece. `equip_type` is `2` for a shield, `3` for a helmet and `4` for body armor.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct Armor {
    #[serde(serialize_with = "serialize_name", deserialize_with = "deserialize_name")]
    pub name: [u8; NAME_BYTES],
    pub equipability: u8,
    pub equip_type: u8,
    pub weight: u8,
    pub power: u8,
    pub price: u16,
}

/// One accessory.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct Accessory {
    #[serde(serialize_with = "serialize_name", deserialize_with = "deserialize_name")]
    pub name: [u8; NAME_BYTES],
    pub equipability: u8,
    pub weight: u8,
    pub price: u16,
}

/// One item a shop sells: the item type (`0` item, `1` weapon, `2` armor, `3` accessory, `4` vital item,
/// `0xFF` empty) and the index in that type's table.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShopSlot {
    pub item_type: u8,
    pub item_index: u8,
}

/// One shop: the number of stocked items and the slots that name them.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct Shop {
    pub count: u8,
    pub slots: [ShopSlot; SHOP_SLOTS],
}

/// One ability (a skill or a spell). An ability type of `3` marks a skill.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct Ability {
    #[serde(serialize_with = "serialize_name", deserialize_with = "deserialize_name")]
    pub name: [u8; NAME_BYTES],
    /// Targeting and display bits; bit 5 is `default_target_enemy`.
    pub flags: u8,
    /// The ability type; its low two bits select the rank bonus.
    pub r#type: u8,
    pub cost: u8,
    pub power: u8,
    /// Element bits: `fire`, `ice`, `lightning`, `earth`, `wind`, `holy`, `psionic`, `status`.
    pub element: u8,
    /// Bits: `affects_stats`, `examinable`, `target_ally_default`.
    pub ability_flags: u8,
}

/// One level of one character's growth. Each character has 99 records, one per level.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct Growth {
    pub exp: u16,
    pub hp: u8,
    pub ap: u8,
    pub power: u8,
    pub defense: u8,
    pub agility: u8,
    pub intellect: u8,
    /// The ability index learned at this level; `0` learns none.
    pub ability: u8,
    /// The second ability-related byte, which the decompilation passes to the same registration call.
    pub ability_candidate: u8,
}

/// Every table of `ETC/GAME.EMI` entry 0.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default, Eq, PartialEq)]
pub struct GameTables {
    pub items: Vec<Item>,
    pub vital_items: Vec<VitalItem>,
    pub weapons: Vec<Weapon>,
    pub armor: Vec<Armor>,
    pub accessories: Vec<Accessory>,
    pub shops: Vec<Shop>,
    pub abilities: Vec<Ability>,
    pub level_growth: Vec<Growth>,
}

impl GameTables {
    /// Reads every table from the payload of entry 0.
    pub fn parse(payload: &[u8]) -> Result<Self> {
        Ok(Self {
            items: ITEMS.records(payload)?.into_iter().map(item).collect(),
            vital_items: VITAL_ITEMS.records(payload)?.into_iter().map(vital_item).collect(),
            weapons: WEAPONS.records(payload)?.into_iter().map(weapon).collect(),
            armor: ARMOR.records(payload)?.into_iter().map(armor).collect(),
            accessories: ACCESSORIES.records(payload)?.into_iter().map(accessory).collect(),
            shops: SHOPS.records(payload)?.into_iter().map(shop).collect(),
            abilities: ABILITIES.records(payload)?.into_iter().map(ability).collect(),
            level_growth: LEVEL_GROWTH.records(payload)?.into_iter().map(growth).collect(),
        })
    }
}

fn name(record: &[u8]) -> [u8; NAME_BYTES] {
    bytes(record, 0)
}

fn item(record: &[u8]) -> Item {
    Item { name: name(record), flags: record[0x0c], price: u16_at(record, 0x10) }
}

fn vital_item(record: &[u8]) -> VitalItem {
    VitalItem { name: name(record) }
}

fn weapon(record: &[u8]) -> Weapon {
    Weapon { name: name(record), equipability: record[0x0c], element: record[0x0f], weight: record[0x10], power: record[0x12], price: u16_at(record, 0x16) }
}

fn armor(record: &[u8]) -> Armor {
    Armor { name: name(record), equipability: record[0x0c], equip_type: record[0x0e], weight: record[0x0f], power: record[0x10], price: u16_at(record, 0x14) }
}

fn accessory(record: &[u8]) -> Accessory {
    Accessory { name: name(record), equipability: record[0x0c], weight: record[0x0f], price: u16_at(record, 0x12) }
}

fn shop(record: &[u8]) -> Shop {
    let slots = std::array::from_fn(|slot| ShopSlot { item_type: record[1 + 2 * slot], item_index: record[2 + 2 * slot] });
    Shop { count: record[0], slots }
}

fn ability(record: &[u8]) -> Ability {
    Ability { name: name(record), flags: record[0x0c], r#type: record[0x0d], cost: record[0x0e], power: record[0x0f], element: record[0x10], ability_flags: record[0x11] }
}

fn growth(record: &[u8]) -> Growth {
    Growth {
        exp: u16_at(record, 0),
        hp: record[2],
        ap: record[3],
        power: record[4] >> 4,
        defense: record[4] & 0x0F,
        agility: record[5] >> 4,
        intellect: record[5] & 0x0F,
        ability: record[6],
        ability_candidate: record[7],
    }
}

/// Writes a name into a record: its stored bytes, colour codes included.
fn put_name(record: &mut [u8], name: &[u8; NAME_BYTES]) {
    record[..NAME_BYTES].copy_from_slice(name);
}

fn put_price(record: &mut [u8], at: usize, price: u16) {
    record[at..at + 2].copy_from_slice(&price.to_le_bytes());
}

/// Writes one item's named bytes into its record. Bytes no field names keep their stored values.
pub fn put_item(record: &mut [u8], item: &Item) -> std::result::Result<(), String> {
    put_name(record, &item.name);
    record[0x0c] = item.flags;
    put_price(record, 0x10, item.price);
    Ok(())
}

/// Writes one vital item's name into its record.
pub fn put_vital_item(record: &mut [u8], item: &VitalItem) -> std::result::Result<(), String> {
    put_name(record, &item.name);
    Ok(())
}

/// Writes one weapon's named bytes into its record.
pub fn put_weapon(record: &mut [u8], weapon: &Weapon) -> std::result::Result<(), String> {
    put_name(record, &weapon.name);
    record[0x0c] = weapon.equipability;
    record[0x0f] = weapon.element;
    record[0x10] = weapon.weight;
    record[0x12] = weapon.power;
    put_price(record, 0x16, weapon.price);
    Ok(())
}

/// Writes one armor piece's named bytes into its record.
pub fn put_armor(record: &mut [u8], armor: &Armor) -> std::result::Result<(), String> {
    put_name(record, &armor.name);
    record[0x0c] = armor.equipability;
    record[0x0e] = armor.equip_type;
    record[0x0f] = armor.weight;
    record[0x10] = armor.power;
    put_price(record, 0x14, armor.price);
    Ok(())
}

/// Writes one accessory's named bytes into its record.
pub fn put_accessory(record: &mut [u8], accessory: &Accessory) -> std::result::Result<(), String> {
    put_name(record, &accessory.name);
    record[0x0c] = accessory.equipability;
    record[0x0f] = accessory.weight;
    put_price(record, 0x12, accessory.price);
    Ok(())
}

/// Writes one shop's stock into its record.
pub fn put_shop(record: &mut [u8], shop: &Shop) -> std::result::Result<(), String> {
    record[0] = shop.count;
    for (slot, stock) in shop.slots.iter().enumerate() {
        record[1 + 2 * slot] = stock.item_type;
        record[2 + 2 * slot] = stock.item_index;
    }
    Ok(())
}

/// Writes one ability's named bytes into its record.
pub fn put_ability(record: &mut [u8], ability: &Ability) -> std::result::Result<(), String> {
    put_name(record, &ability.name);
    record[0x0c] = ability.flags;
    record[0x0d] = ability.r#type;
    record[0x0e] = ability.cost;
    record[0x0f] = ability.power;
    record[0x10] = ability.element;
    record[0x11] = ability.ability_flags;
    Ok(())
}

/// Writes one level's growth into its record. The four stats are nibbles, so each must be 15 or less.
pub fn put_growth(record: &mut [u8], growth: &Growth) -> std::result::Result<(), String> {
    if [growth.power, growth.defense, growth.agility, growth.intellect].iter().any(|&stat| stat > 0x0F) {
        return Err("a growth stat is a nibble, from 0 to 15".into());
    }
    record[0..2].copy_from_slice(&growth.exp.to_le_bytes());
    record[2] = growth.hp;
    record[3] = growth.ap;
    record[4] = (growth.power << 4) | growth.defense;
    record[5] = (growth.agility << 4) | growth.intellect;
    record[6] = growth.ability;
    record[7] = growth.ability_candidate;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A payload that spans every table, with one item, "Green Apple", at its place, and the rest zero.
    fn payload() -> Vec<u8> {
        let end = TABLES.iter().map(|table| table.archive_offset - PAYLOAD_START + table.count * table.record).max();
        let mut payload = vec![0u8; end.expect("there are tables")];
        let at = ITEMS.archive_offset - PAYLOAD_START + ITEMS.record;
        payload[at..at + 6].copy_from_slice(b"Green\xff");
        payload[at + 6..at + 11].copy_from_slice(b"Apple");
        payload[at + 0x0c] = 0x21;
        payload[at + 0x10..at + 0x12].copy_from_slice(&120u16.to_le_bytes());
        payload
    }

    #[test]
    fn an_item_reads_its_name_flags_and_price() {
        let tables = GameTables::parse(&payload()).unwrap();
        assert_eq!(tables.items.len(), 92);
        assert_eq!(name_text(&tables.items[1].name), "Green Apple");
        assert_eq!((tables.items[1].flags, tables.items[1].price), (0x21, 120));
    }

    #[test]
    fn a_short_payload_names_the_table_it_cannot_read() {
        let short = &payload()[..ITEMS.archive_offset - PAYLOAD_START + 10];
        let error = GameTables::parse(short).unwrap_err();
        assert!(matches!(error, Error::GameTableShort { table: "items", .. }), "{error:?}");
    }

    #[test]
    fn a_growth_byte_holds_two_nibbles() {
        let growth = growth(&[0x10, 0x27, 9, 4, 0x3C, 0xA7, 2, 5]);
        assert_eq!((growth.exp, growth.hp, growth.ap), (10_000, 9, 4));
        assert_eq!((growth.power, growth.defense), (3, 12));
        assert_eq!((growth.agility, growth.intellect), (10, 7));
    }

    #[test]
    fn a_name_ends_at_its_zero_and_spaces_its_ff() {
        assert_eq!(name_text(b"Bronze\xffSword\0\0\0\0\0\0"), "Bronze Sword");
        assert_eq!(name_text(&[0x41, 0x80, 0, 0]), "A{RAW:80}");
    }

    #[test]
    fn a_name_uses_the_table_punctuation_and_keeps_colour_codes() {
        assert_eq!(name_text(b"Man=o\x8e=War\0\0"), "Man-o'-War");
        assert_eq!(name_text(b"\x05\x02Hawk\x8es\xff\x8b\x06\0"), "{COLOR:02}Hawk's +{RAW:06}");
    }

    #[test]
    fn a_name_field_writes_back_the_bytes_it_was_read_from() {
        let fields: [&[u8]; 4] = [b"Bronze\xffSword", b"Potion\0\0\0\0\0\0", &[0x05, 0x02, b'H', b'a', b'w', b'k', 0x8e, b's', 0xff, 0x8b, 0x06, 0x00], &[b'A', 0x00, b'B', 0, 0, 0, 0, 0]];
        for field in fields {
            let text = name_text(field);
            assert_eq!(name_field(&text, field.len()).unwrap(), field, "{text:?}");
        }
    }

    #[test]
    fn a_name_longer_than_its_field_is_refused() {
        assert!(name_field("Longer Than Eight", 8).is_err());
    }
}
