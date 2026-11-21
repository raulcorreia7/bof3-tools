//! The game catalog: every entry of every archive, filed the way the game's developers keep their
//! source assets (characters, enemies, areas, battle, interface, audio, text, data tables).
//!
//! The grouping is derived, not listed per entry. An entry is placed by the first rule that matches:
//!
//! 1. a fixed game table: an entry that a [`schema`] sheet is bound to by archive and slot
//!    (`ETC/GAME.EMI` entry 0, `ETC/START.EMI` entry 8) goes to [`Which::DataTables`];
//! 2. its format, for the formats whose owner is not the archive that holds them (see [`RULES`]);
//! 3. its archive's name, by the family rules of [`RULES`];
//! 4. otherwise [`Which::Unsorted`], with the reason in the section title.
//!
//! Every row of [`RULES`] cites its source. Units (archives) sort in the disc's own order: the disc file
//! list is in ascending name order within each folder, which is also the order of the slot ids in
//! `include/sys/cd_format.h` (`EMI_SLOT_*`), and the numbered series count up (`ENEMY000`, `AREA105`).

use std::collections::{BTreeMap, BTreeSet, HashSet};

use rebof3_emi::Archive;
use rebof3_formats::characters;
use rebof3_formats::{Format, classify};

use crate::archive::ArchivePath;
use crate::schema::{self, Place};

/// A top-level group of the catalog, in display order.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Which {
    Characters,
    Enemies,
    Areas,
    Battle,
    Interface,
    Audio,
    Text,
    DataTables,
    Other,
    Unsorted,
}

impl Which {
    /// Every group, in display order.
    pub const ALL: [Which; 10] = [Which::Characters, Which::Enemies, Which::Areas, Which::Battle, Which::Interface, Which::Audio, Which::Text, Which::DataTables, Which::Other, Which::Unsorted];

    /// The group's title.
    pub fn title(self) -> &'static str {
        match self {
            Which::Characters => "Characters",
            Which::Enemies => "Enemies and bosses",
            Which::Areas => "Areas and maps",
            Which::Battle => "Battle",
            Which::Interface => "Interface",
            Which::Audio => "Audio",
            Which::Text => "Text",
            Which::DataTables => "Data tables",
            Which::Other => "Movies and other",
            Which::Unsorted => "Unsorted",
        }
    }
}

/// How a row finds the entries it files.
#[derive(Clone, Copy, Debug)]
enum Pick {
    /// A numbered series: the prefix, digits and at most one trailing capital (`ENEMY005`, `BGM002A`).
    Series(&'static str),
    /// These archive names (without `.EMI`).
    Any(&'static [&'static str]),
    /// Every archive in this disc folder (after the more specific rows).
    Folder(&'static str),
    /// Every entry of this format, in any archive.
    Format(Format),
}

/// How a unit of a row is titled.
#[derive(Clone, Copy, Debug)]
enum Label {
    /// The archive name.
    Stem,
    /// `Area 105 (WORLD02)`: the series number and the disc folder.
    Area,
    /// `Area 5 enemy archive (ENEMY005)`.
    EnemyArchive,
    /// `Boss 12 (BOSS012)`.
    Boss,
    /// `Spell effect 160 (MAGIC160)`.
    Spell,
    /// `PL012: Ryu, Nina, Garr`: the digits of the name read as character ids.
    Members,
}

/// One row of the catalog table: which entries, where they go and why.
struct Rule {
    pick: Pick,
    group: Which,
    /// The section of the group. Rows with the same title share a section.
    section: &'static str,
    label: Label,
    /// The decomp symbol or format doc that places the row.
    source: &'static str,
}

/// The ETC programs by slot: `wiki: formats/emi#code-overlays`.
const ETC: &str = "include/sys/cd_format.h ETC block; wiki: formats/emi";
/// The reason for an archive that no row names.
/// The section title of an archive no row names.
pub const NO_ROW: &str = "no catalog row names this archive";

/// The catalog table, in display order. A format row beats a family row; among family rows the first
/// match wins.
static RULES: &[Rule] = &[
    // Characters: the party's field and battle archives.
    Rule {
        pick: Pick::Series("PL"),
        group: Which::Characters,
        section: "Field archives (PL, digits are character ids)",
        label: Label::Members,
        source: "EMI_SLOT_PL, EMI_FAMILY_PL; wiki: formats/character",
    },
    Rule {
        pick: Pick::Series("PLP"),
        group: Which::Characters,
        section: "Field party code (PLP, digits are character ids)",
        label: Label::Members,
        source: "EMI_SLOT_PLP, EMI_FAMILY_PLP; wiki: formats/character",
    },
    Rule {
        pick: Pick::Series("BPLD"),
        group: Which::Characters,
        section: "Battle archives (BPLD, digits are character ids)",
        label: Label::Members,
        source: "EMI_SLOT_BPLD, EMI_FAMILY_BPLD; wiki: formats/emi",
    },
    Rule {
        pick: Pick::Series("BPLU"),
        group: Which::Characters,
        section: "Battle archives (BPLU, digits are character ids)",
        label: Label::Members,
        source: "EMI_SLOT_BPLU, EMI_FAMILY_BPLU; wiki: formats/emi",
    },
    Rule {
        pick: Pick::Folder("BPLCHAR"),
        group: Which::Characters,
        section: "Battle form archives",
        label: Label::Stem,
        source: "BPLCHAR holds the battle character sprites and forms; wiki: formats/disc",
    },
    // Enemies and bosses.
    Rule {
        pick: Pick::Series("ENEMY"),
        group: Which::Enemies,
        section: "Enemy archives, one per area",
        label: Label::EnemyArchive,
        source: "EMI_SLOT_ENEMY; the battle start streams slot 9 + g_GameAreaId (wiki: formats/emi)",
    },
    Rule { pick: Pick::Series("BOSS"), group: Which::Enemies, section: "Boss battles", label: Label::Boss, source: "EMI_SLOT_BOSS" },
    Rule {
        pick: Pick::Format(Format::AreaEncounterTable),
        group: Which::Enemies,
        section: "Formations (encounter tables of the areas)",
        label: Label::Area,
        source: "the area encounter table is the formation data; rebof3_formats::encounter",
    },
    // Areas and maps.
    Rule { pick: Pick::Series("AREA"), group: Which::Areas, section: "Areas", label: Label::Area, source: "EMI_SLOT_AREA: one archive per area, slot area + 0x2AB" },
    // Battle.
    Rule {
        pick: Pick::Any(&["BATL_DRA", "BATL_END", "BATL_OVR", "BATL_RE2", "BATL_RET", "BATL_SE", "BATTLE", "BATTLE2", "COMN_SE"]),
        group: Which::Battle,
        section: "Battle overlays, backgrounds and effects",
        label: Label::Stem,
        source: "EMI_SLOT_BATL_DRA to EMI_SLOT_COMN_SE (the BATTLE block)",
    },
    Rule { pick: Pick::Series("MAGIC"), group: Which::Battle, section: "Magic effects", label: Label::Spell, source: "EMI_SLOT_MAGIC; BMAGIC holds the spell effect overlays (wiki: formats/disc)" },
    Rule {
        pick: Pick::Any(&["KAIZAR_D", "KAIZAR_F", "KAIZAR_N"]),
        group: Which::Battle,
        section: "Magic effects",
        label: Label::Stem,
        source: "BMAGIC holds the three KAIZAR effect blocks (wiki: formats/disc)",
    },
    Rule { pick: Pick::Any(&["SCE10EFF"]), group: Which::Battle, section: "Scenario effects", label: Label::Stem, source: "SCE10EFF, SCE15EF0-3 are effect controllers (wiki: formats/emi)" },
    Rule { pick: Pick::Series("SCE15EF"), group: Which::Battle, section: "Scenario effects", label: Label::Stem, source: "SCE10EFF, SCE15EF0-3 are effect controllers (wiki: formats/emi)" },
    // Interface.
    Rule {
        pick: Pick::Format(Format::AreaAuxWindow),
        group: Which::Interface,
        section: "Windows (auxiliary window modules of the areas)",
        label: Label::Area,
        source: "rebof3_formats::window, the auxiliary module window",
    },
    Rule {
        pick: Pick::Any(&["START", "STATUS", "SHOP"]),
        group: Which::Interface,
        section: "Menus",
        label: Label::Stem,
        source: "EMI_SLOT_START, EMI_SLOT_STATUS, EMI_SLOT_SHOP; wiki: formats/emi (ETC programs)",
    },
    Rule {
        pick: Pick::Any(&["FIRST", "ENDKANJI"]),
        group: Which::Interface,
        section: "Boot screens and font sheets",
        label: Label::Stem,
        source: "FIRST#3 and ENDKANJI#0 are the font sheet (wiki: guides/graphics)",
    },
    Rule {
        pick: Pick::Any(&["GAME"]),
        group: Which::Interface,
        section: "Game program and title",
        label: Label::Stem,
        source: "EMI_SLOT_GAME holds the resident program and the title overlay (wiki: concepts/architecture)",
    },
    // Audio.
    Rule { pick: Pick::Series("BGM"), group: Which::Audio, section: "Music", label: Label::Stem, source: "EMI_SLOT_BGM" },
    Rule { pick: Pick::Series("BGMBAT"), group: Which::Audio, section: "Music", label: Label::Stem, source: "BGMBAT00-06 are the battle music files of the BGM folder" },
    Rule { pick: Pick::Any(&["BGMEND", "BGMOPN", "BGMSPC"]), group: Which::Audio, section: "Music", label: Label::Stem, source: "the BGM folder" },
    Rule { pick: Pick::Any(&["A085SE2", "A108SE2"]), group: Which::Audio, section: "Area sound effects", label: Label::Stem, source: "EMI_SLOT_A085SE2, EMI_SLOT_A108SE2; wiki: guides/sound" },
    // Text.
    Rule {
        pick: Pick::Format(Format::SharedResourceTable),
        group: Which::Text,
        section: "Shared strings (resource table at 0x8001A000)",
        label: Label::Stem,
        source: "rebof3_formats::resources; wiki: data/text#references",
    },
    Rule {
        pick: Pick::Any(&["AFLDKWA"]),
        group: Which::Text,
        section: "Shared strings (resource table at 0x8001A000)",
        label: Label::Stem,
        source: "AFLDKWA#0 is a shared resource table (wiki: guides/text)",
    },
    // Movies and other.
    Rule { pick: Pick::Any(&["DEMO"]), group: Which::Other, section: "Demo", label: Label::Stem, source: "EMI_SLOT_DEMO" },
    Rule { pick: Pick::Series("SCENA"), group: Which::Other, section: "Scenario overlays", label: Label::Stem, source: "EMI_SLOT_SCENA: slot scenario_id + 661 (wiki: formats/disc)" },
    Rule { pick: Pick::Series("COMMU"), group: Which::Other, section: "ETC programs", label: Label::Stem, source: ETC },
    Rule { pick: Pick::Any(&["BATE", "SHISU", "SISYOU"]), group: Which::Other, section: "ETC programs", label: Label::Stem, source: ETC },
    // Unsorted: placed by the table, but with no known place.
    Rule {
        pick: Pick::Any(&["MTEST", "RTEST"]),
        group: Which::Unsorted,
        section: "No caller found in the decomp",
        label: Label::Stem,
        source: "EMI_SLOT_MTEST, EMI_SLOT_RTEST have no caller (wiki: appendix/open-questions)",
    },
];

/// The names of the stored character ids, `CharacterId` in `include/game/character.h`; the index is the id.
const CHARACTER_IDS: [&str; 11] = ["Ryu (young)", "Nina (young)", "Garr", "Teepo", "Rei", "Momo", "Peco", "Ryu (adult)", "Nina (adult)", "Ryu (pajamas)", "Whelp"];

/// One entry as the catalog places it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Placed {
    /// The entry's slot in its archive.
    pub index: usize,
    pub format: Format,
    /// What the entry holds beyond its format, when a sheet or a decoded name says so; else empty.
    pub note: String,
}

/// An archive's entries that one section holds.
#[derive(Clone, Debug)]
pub struct Unit {
    pub archive: ArchivePath,
    pub title: String,
    pub entries: Vec<Placed>,
}

/// A section of a group: a family of archives, or a kind of entry.
#[derive(Clone, Debug)]
pub struct Section {
    pub title: &'static str,
    /// The decomp symbol or format doc that files the section.
    pub source: &'static str,
    pub units: Vec<Unit>,
}

/// A top-level group.
#[derive(Clone, Debug)]
pub struct Group {
    pub which: Which,
    pub sections: Vec<Section>,
}

impl Group {
    /// The entries the group holds.
    pub fn entries(&self) -> usize {
        self.sections.iter().map(Section::entries).sum()
    }

    /// The archives the group holds entries of, an archive counted once.
    pub fn archives(&self) -> usize {
        self.sections.iter().flat_map(|section| section.units.iter().map(|unit| &unit.archive)).collect::<BTreeSet<_>>().len()
    }
}

impl Section {
    /// The entries the section holds.
    pub fn entries(&self) -> usize {
        self.units.iter().map(|unit| unit.entries.len()).sum()
    }
}

/// What the scan read of one entry.
#[derive(Clone, Debug)]
pub struct ScannedEntry {
    pub format: Format,
    /// A fixed game table: the names of the sheets bound to this entry, plus decoded names.
    pub table: Option<String>,
}

/// What the scan read of one archive: the formats of its entries.
#[derive(Clone, Debug)]
pub struct Scanned {
    pub path: ArchivePath,
    pub entries: Vec<ScannedEntry>,
}

/// What the scan reads of one archive: its entries' formats and fixed-table notes.
pub fn scan_archive(path: &ArchivePath, bytes: &[u8]) -> Result<Scanned, rebof3_emi::Error> {
    let archive = Archive::parse(bytes)?;
    let sheets: Vec<_> = schema::sheets().into_iter().filter(|sheet| matches!(sheet.place, Place::Entry { .. })).collect();
    let entries = (0..archive.entries().len())
        .map(|index| {
            let payload = archive.payload(bytes, index).unwrap_or(&[]);
            let format = classify(&archive.entries()[index], payload);
            let names: Vec<_> = sheets.iter().filter(|sheet| sheet.applies(&path.0, index, format)).map(|sheet| sheet.name).collect();
            let table = (!names.is_empty()).then(|| table_note(&path.0, index, &names, payload));
            ScannedEntry { format, table }
        })
        .collect();
    Ok(Scanned { path: path.clone(), entries })
}

/// The note of a fixed table entry: its sheets, and the party's names where the entry holds them.
fn table_note(archive: &str, index: usize, sheets: &[&str], payload: &[u8]) -> String {
    let mut note = sheets.join(", ");
    if archive == schema::START
        && index == schema::START_PARTY_ENTRY
        && let Ok(party) = characters::parse(payload)
    {
        let names: Vec<_> = party.iter().map(|character| character.name.as_str()).collect();
        note = format!("{note}: {}", names.join(", "));
    }
    note
}

/// The catalog: every scanned entry, filed once.
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    pub groups: Vec<Group>,
}

/// Where a placement files an entry, before units are built.
struct Placement {
    group: Which,
    /// The index of the row in [`RULES`]; `RULES.len()` for the fixed tables and the no-row fallback.
    row: usize,
    section: &'static str,
    source: &'static str,
    label: Label,
    /// The series prefix of the row, or empty.
    prefix: &'static str,
}

impl Catalog {
    /// Files every scanned entry. `scanned` may list the archives in any order.
    pub fn build(scanned: &[Scanned]) -> Self {
        // (group, first row, section) -> unit key -> unit
        type Key = (Which, usize, &'static str);
        let mut sections: BTreeMap<Key, (&'static str, BTreeMap<UnitKey, Unit>)> = BTreeMap::new();

        for archive in scanned {
            let (parent, file) = archive.path.split();
            let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
            for (index, entry) in archive.entries.iter().enumerate() {
                let place = place(parent, stem, entry);
                let key = (place.group, section_order(place.group, place.section, place.row), place.section);
                let (_, units) = sections.entry(key).or_insert_with(|| (place.source, BTreeMap::new()));
                let unit_key = UnitKey::new(place.row, stem, &archive.path);
                let unit = units.entry(unit_key).or_insert_with(|| Unit { archive: archive.path.clone(), title: title(place.label, place.prefix, parent, stem), entries: Vec::new() });
                unit.entries.push(Placed { index, format: entry.format, note: entry.table.clone().unwrap_or_default() });
            }
        }

        let mut groups: Vec<Group> = Which::ALL.iter().map(|which| Group { which: *which, sections: Vec::new() }).collect();
        for ((group, _, title), (source, units)) in sections {
            let at = Which::ALL.iter().position(|which| *which == group).expect("every group is listed");
            groups[at].sections.push(Section { title, source, units: units.into_values().collect() });
        }
        groups.retain(|group| !group.sections.is_empty());
        Self { groups }
    }

    /// Every entry of the catalog, as `(archive, index)`, in display order.
    pub fn placements(&self) -> impl Iterator<Item = (&ArchivePath, usize)> {
        self.groups.iter().flat_map(|group| group.sections.iter().flat_map(|section| section.units.iter().flat_map(|unit| unit.entries.iter().map(move |entry| (&unit.archive, entry.index)))))
    }

    /// The entries the catalog holds.
    pub fn entries(&self) -> usize {
        self.groups.iter().map(Group::entries).sum()
    }
}

/// The order of a section among its group's: the row that first names it, so rows sharing a title
/// share a section.
fn section_order(group: Which, section: &str, row: usize) -> usize {
    RULES.iter().position(|candidate| candidate.group == group && candidate.section == section).unwrap_or(row)
}

/// The sort key of a unit: its row, the series number and suffix, then the path.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct UnitKey {
    row: usize,
    number: u32,
    suffix: String,
    path: String,
}

impl UnitKey {
    fn new(row: usize, stem: &str, path: &ArchivePath) -> Self {
        let (number, suffix) = RULES
            .get(row)
            .and_then(|row| match row.pick {
                Pick::Series(prefix) => series(stem, prefix),
                _ => None,
            })
            .unwrap_or((0, String::new()));
        Self { row, number, suffix, path: path.0.clone() }
    }
}

/// The number and the trailing capital of `stem` when it is `prefix`, digits and at most one capital.
fn series(stem: &str, prefix: &str) -> Option<(u32, String)> {
    let rest = stem.strip_prefix(prefix)?;
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    let suffix = &rest[digits..];
    let capital = suffix.len() <= 1 && suffix.chars().all(|letter| letter.is_ascii_uppercase());
    (digits > 0 && capital).then(|| (rest[..digits].parse().unwrap_or(0), suffix.to_string()))
}

/// The placement of one entry.
fn place(folder: &str, stem: &str, entry: &ScannedEntry) -> Placement {
    if entry.table.is_some() {
        return Placement {
            group: Which::DataTables,
            row: RULES.len(),
            section: "Fixed tables",
            source: "schema sheets bound to one entry: ETC/GAME.EMI#0 (rebof3_formats::game_tables), ETC/START.EMI#8 (characters)",
            label: Label::Stem,
            prefix: "",
        };
    }
    let by_format = RULES.iter().enumerate().find(|(_, row)| matches!(row.pick, Pick::Format(format) if format == entry.format));
    let by_name = || {
        RULES.iter().enumerate().find(|(_, row)| match row.pick {
            Pick::Series(prefix) => series(stem, prefix).is_some(),
            Pick::Any(names) => names.contains(&stem),
            Pick::Folder(name) => folder.rsplit('/').next() == Some(name),
            Pick::Format(_) => false,
        })
    };
    match by_format.or_else(by_name) {
        Some((at, row)) => {
            let prefix = if let Pick::Series(prefix) = row.pick { prefix } else { "" };
            Placement { group: row.group, row: at, section: row.section, source: row.source, label: row.label, prefix }
        }
        None => Placement { group: Which::Unsorted, row: RULES.len(), section: NO_ROW, source: NO_ROW, label: Label::Stem, prefix: "" },
    }
}

/// A unit's title. `prefix` is the series prefix of the row that filed the archive.
fn title(label: Label, prefix: &str, folder: &str, stem: &str) -> String {
    let number = |prefix: &str| series(stem, prefix).map(|(number, suffix)| format!("{number}{suffix}"));
    let world = folder.rsplit('/').next().unwrap_or(folder);
    let numbered = |text: Option<String>| text.unwrap_or_else(|| stem.to_string());
    match label {
        Label::Stem => stem.to_string(),
        Label::Area => numbered(number("AREA").map(|n| format!("Area {n} ({world})"))),
        Label::EnemyArchive => numbered(number("ENEMY").map(|n| format!("Area {n} enemy archive ({stem})"))),
        Label::Boss => numbered(number("BOSS").map(|n| format!("Boss {n} ({stem})"))),
        Label::Spell => numbered(number("MAGIC").map(|n| format!("Spell effect {n} ({stem})"))),
        Label::Members => numbered(members(stem, prefix).map(|names| format!("{stem}: {names}"))),
    }
}

/// The party an archive name spells: each character after the prefix is a character id in hex.
fn members(stem: &str, prefix: &str) -> Option<String> {
    let names: Option<Vec<&str>> = stem.strip_prefix(prefix)?.chars().map(|digit| digit.to_digit(16).and_then(|id| CHARACTER_IDS.get(id as usize).copied())).collect();
    names.filter(|names| !names.is_empty()).map(|names| names.join(", "))
}

/// One line of the catalog tree.
#[derive(Clone, Debug)]
pub struct Line {
    /// How deep the line sits: group 0, section 1, archive 2, entry 3.
    pub depth: usize,
    /// A key that is stable while the catalog is, for the open state and the element id.
    pub key: String,
    pub label: String,
    /// The entries under the line (1 for an entry).
    pub count: usize,
    /// Whether a line with children is open; `None` for an entry.
    pub open: Option<bool>,
    /// What a click opens: the archive, and the entry when the line is one.
    pub archive: Option<ArchivePath>,
    pub entry: Option<usize>,
    pub format: Option<Format>,
    /// The line's source or note, for the tooltip.
    pub tip: String,
}

/// The tree's filter and open keys, shared by the levels that build their lines.
struct Walk<'a> {
    filter: String,
    open: &'a HashSet<String>,
}

/// The lines of one level and whether anything under it matched the filter.
struct Level {
    rows: Vec<Line>,
    inner: bool,
}

impl Walk<'_> {
    fn filtering(&self) -> bool {
        !self.filter.is_empty()
    }

    fn hit(&self, text: &str) -> bool {
        self.filtering() && text.to_ascii_uppercase().contains(&self.filter)
    }

    /// A line is open when the user opened it or a filter match below forces it.
    fn is_open(&self, key: &str, forced: bool) -> bool {
        forced || self.open.contains(key)
    }

    fn unit_level(&self, section_key: &str, section: &Section, anchor_hit: bool) -> Level {
        let mut level = Level { rows: Vec::new(), inner: false };
        for unit in &section.units {
            let unit_key = format!("{section_key}/{}", unit.archive.0);
            let unit_hit = self.hit(&unit.title) || self.hit(&unit.archive.0);
            let matching: Vec<&Placed> = unit.entries.iter().filter(|entry| self.hit(&entry_label(entry))).collect();
            if self.filtering() && !(anchor_hit || unit_hit) && matching.is_empty() {
                continue;
            }
            level.inner |= unit_hit || !matching.is_empty();
            let forced = self.filtering() && !matching.is_empty();
            let is_open = self.is_open(&unit_key, forced);
            level.rows.push(Line {
                depth: 2,
                key: unit_key.clone(),
                label: unit.title.clone(),
                count: unit.entries.len(),
                open: Some(is_open),
                archive: Some(unit.archive.clone()),
                entry: None,
                format: None,
                tip: unit.archive.0.clone(),
            });
            if is_open {
                let shown: Vec<&Placed> = if forced && !unit_hit && !anchor_hit { matching } else { unit.entries.iter().collect() };
                level.rows.extend(shown.into_iter().map(|entry| Line {
                    depth: 3,
                    key: format!("{unit_key}#{}", entry.index),
                    label: entry_label(entry),
                    count: 1,
                    open: None,
                    archive: Some(unit.archive.clone()),
                    entry: Some(entry.index),
                    format: Some(entry.format),
                    tip: entry.note.clone(),
                }));
            }
        }
        level
    }

    fn section_level(&self, group_key: &str, group: &Group, group_hit: bool) -> Level {
        let mut level = Level { rows: Vec::new(), inner: false };
        for section in &group.sections {
            let section_key = format!("{group_key}/{}", section.title);
            let section_hit = self.hit(section.title);
            let units = self.unit_level(&section_key, section, group_hit || section_hit);
            if units.rows.is_empty() {
                continue;
            }
            level.inner |= section_hit || units.inner;
            let is_open = self.is_open(&section_key, self.filtering() && units.inner);
            level.rows.push(Line {
                depth: 1,
                key: section_key,
                label: section.title.to_string(),
                count: section.entries(),
                open: Some(is_open),
                archive: None,
                entry: None,
                format: None,
                tip: section.source.to_string(),
            });
            if is_open {
                level.rows.extend(units.rows);
            }
        }
        level
    }
}

impl Catalog {
    /// The lines of the tree. `open` holds the keys of the lines the user opened. A filter keeps the
    /// lines whose text matches, with their parents, and opens the parents; the contents of a line that
    /// matches stay shut until opened.
    pub fn lines(&self, filter: &str, open: &HashSet<String>) -> Vec<Line> {
        let walk = Walk { filter: filter.to_ascii_uppercase(), open };
        let mut rows = Vec::new();
        for group in &self.groups {
            let group_key = format!("g:{}", group.which.title());
            let sections = walk.section_level(&group_key, group, walk.hit(group.which.title()));
            if sections.rows.is_empty() {
                continue;
            }
            let is_open = walk.is_open(&group_key, walk.filtering() && sections.inner);
            rows.push(Line {
                depth: 0,
                key: group_key,
                label: group.which.title().to_string(),
                count: group.entries(),
                open: Some(is_open),
                archive: None,
                entry: None,
                format: None,
                tip: format!("{} archives", group.archives()),
            });
            if is_open {
                rows.extend(sections.rows);
            }
        }
        rows
    }
}

/// The text of an entry's line: its slot, its format and its note.
fn entry_label(entry: &Placed) -> String {
    if entry.note.is_empty() { format!("{:>2}  {}", entry.index, entry.format.name()) } else { format!("{:>2}  {}: {}", entry.index, entry.format.name(), entry.note) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_rule_cites_its_source() {
        for rule in RULES {
            assert!(!rule.source.is_empty() && !rule.section.is_empty());
        }
    }
}
