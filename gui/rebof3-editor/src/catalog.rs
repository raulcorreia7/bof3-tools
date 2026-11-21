//! The catalog as the editor opens it: the rules of [`rebof3_catalog::catalog`], the scan of the disc
//! folder and the panel mode the project folder remembers.

use std::fs;
use std::path::Path;

pub use rebof3_catalog::catalog::*;

use rebof3_catalog::archive::ArchivePath;

/// Reads every archive once and keeps only what the catalog needs, so no archive stays in memory.
pub fn scan(disc: &Path, archives: &[ArchivePath]) -> Result<Vec<Scanned>, String> {
    archives
        .iter()
        .map(|path| {
            let bytes = fs::read(disc.join(&path.0)).map_err(|error| format!("{}: {error}", path.0))?;
            scan_archive(path, &bytes).map_err(|error| format!("{}: {error}", path.0))
        })
        .collect()
}

/// Which tree the left panel shows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    /// The files of the disc, folder by folder.
    Disc,
    /// The game catalog.
    Game,
}

/// The file in the project folder that remembers the panel's mode.
const MODE_FILE: &str = "panel-mode";

impl Mode {
    /// The mode the project folder remembers; the catalog when it remembers none.
    pub fn read(folder: &Path) -> Self {
        match fs::read_to_string(folder.join(MODE_FILE)).as_deref().map(str::trim) {
            Ok("disc") => Mode::Disc,
            _ => Mode::Game,
        }
    }

    /// Remembers the mode in the project folder. A folder that cannot be written keeps the mode for the session only.
    pub fn write(self, folder: &Path) {
        let _ = fs::write(folder.join(MODE_FILE), if self == Mode::Disc { "disc\n" } else { "game\n" });
    }

    /// The label on the panel's toggle.
    pub fn label(self) -> &'static str {
        match self {
            Mode::Disc => "Disc",
            Mode::Game => "Game",
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, HashSet};

    use rebof3_formats::Format;

    use super::*;
    use crate::fixture::{Disc, Entry};
    use crate::project::Project;
    use crate::schema;

    /// A payload that is not a reserved slot.
    const BYTES: &[u8] = b"data";
    /// The load arguments that the classifier reads as a format (`rebof3_formats::classify`).
    const MAP_BUNDLE: u32 = 0x8010_4000;
    const ENCOUNTERS: u32 = 0x800e_4000;
    const MESSAGES: u32 = 0x8001_0000;
    const WINDOW: u32 = 0x8011_7000;
    const SHARED: u32 = 0x8001_a000;
    const GAME_CODE: u32 = 0x8019_5800;

    fn entry(file_type: u16, ram_ptr: u32) -> Entry<'static> {
        Entry { file_type, ram_ptr, bytes: BYTES }
    }

    /// A disc of one archive of each kind of rule, and one no rule names.
    fn disc(name: &str) -> Disc {
        let area = [entry(0, MAP_BUNDLE), entry(0, ENCOUNTERS), entry(0, MESSAGES), entry(0, WINDOW)];
        let sound = [entry(6, 0), entry(8, 0), entry(7, 0)];
        let game = [entry(0, GAME_CODE), entry(0, 0x801d_0c00)];
        let shared = [entry(0, SHARED)];
        let plain = [entry(2, 0)];
        Disc::with_archives(
            name,
            &[
                ("BIN/WORLD00/AREA002.EMI", &area[..]),
                ("BIN/WORLD00/AREA010.EMI", &area[..]),
                ("BIN/WORLD01/AREA100.EMI", &area[..]),
                ("BIN/BENEMY/ENEMY001.EMI", &sound[..]),
                ("BIN/BGM/BGM002A.EMI", &sound[..]),
                ("BIN/PLCHAR/PL012.EMI", &sound[..]),
                ("BIN/PLCHAR/PL27A.EMI", &sound[..]),
                ("BIN/ETC/GAME.EMI", &game[..]),
                ("BIN/ETC/AFLDKWA.EMI", &shared[..]),
                ("BIN/ETC/MTEST.EMI", &plain[..]),
                ("BIN/ETC/ZZZZ.EMI", &plain[..]),
            ],
        )
    }

    fn catalog(disc: &Disc) -> Catalog {
        let project = Project::open(&disc.disc(), &disc.project()).expect("project opens");
        Catalog::build(&scan(&disc.disc(), &project.archives).expect("scan reads"))
    }

    fn titles(catalog: &Catalog, which: Which) -> Vec<String> {
        let group = catalog.groups.iter().find(|group| group.which == which).expect("the group exists");
        group.sections.iter().flat_map(|section| section.units.iter().map(|unit| unit.title.clone())).collect()
    }

    #[test]
    fn every_entry_is_filed_exactly_once() {
        let disc = disc("catalog-once");
        let catalog = catalog(&disc);

        let mut seen = BTreeSet::new();
        for (archive, index) in catalog.placements() {
            assert!(seen.insert((archive.clone(), index)), "{} entry {index} is filed twice", archive.0);
        }
        let project = Project::open(&disc.disc(), &disc.project()).expect("project opens");
        let expected = scan(&disc.disc(), &project.archives).expect("scan reads").iter().map(|archive| archive.entries.len()).sum::<usize>();
        assert_eq!((seen.len(), catalog.entries()), (expected, expected));
        assert_eq!(expected, 3 * 4 + 3 + 3 + 3 + 3 + 2 + 1 + 1 + 1);
    }

    #[test]
    fn archives_sort_in_the_games_order_and_carry_the_games_numbers() {
        let catalog = catalog(&disc("catalog-order"));

        assert_eq!(titles(&catalog, Which::Areas), ["Area 2 (WORLD00)", "Area 10 (WORLD00)", "Area 100 (WORLD01)"]);
        assert_eq!(titles(&catalog, Which::Enemies)[0], "Area 1 enemy archive (ENEMY001)");
        assert_eq!(titles(&catalog, Which::Characters), ["PL012: Ryu (young), Nina (young), Garr", "PL27A: Garr, Ryu (adult), Whelp"]);
        assert_eq!(titles(&catalog, Which::Audio), ["BGM002A"]);
    }

    #[test]
    fn formats_that_belong_elsewhere_leave_their_archive() {
        let catalog = catalog(&disc("catalog-formats"));

        // The encounter tables are the formations; the auxiliary windows are interface; the shared
        // resource tables are text; the area keeps its terrain and messages.
        assert_eq!(titles(&catalog, Which::Enemies).iter().filter(|title| title.starts_with("Area") && title.contains("WORLD")).count(), 3);
        assert_eq!(titles(&catalog, Which::Interface).len(), 3 + 1);
        assert_eq!(titles(&catalog, Which::Text), ["AFLDKWA"]);
        let areas = catalog.groups.iter().find(|group| group.which == Which::Areas).expect("areas");
        assert!(areas.sections[0].units[0].entries.iter().all(|entry| { !matches!(entry.format, Format::AreaEncounterTable | Format::AreaAuxWindow) }));
    }

    #[test]
    fn a_fixed_table_goes_to_the_data_tables_with_its_sheets() {
        let catalog = catalog(&disc("catalog-tables"));

        let group = catalog.groups.iter().find(|group| group.which == Which::DataTables).expect("data tables");
        let unit = &group.sections[0].units[0];
        assert_eq!(unit.archive.0, schema::GAME);
        assert_eq!(unit.entries.len(), 1);
        assert_eq!(unit.entries[0].index, 0);
        assert!(unit.entries[0].note.contains("items") && unit.entries[0].note.contains("abilities"));
        // GAME.EMI's other entry is the title overlay, filed with the game program.
        assert_eq!(titles(&catalog, Which::Interface).iter().filter(|title| *title == "GAME").count(), 1);
    }

    #[test]
    fn what_no_rule_places_is_unsorted_with_the_reason() {
        let catalog = catalog(&disc("catalog-unsorted"));

        let group = catalog.groups.iter().find(|group| group.which == Which::Unsorted).expect("unsorted");
        let reasons: Vec<_> = group.sections.iter().map(|section| (section.title, titles_of(section))).collect();
        assert_eq!(reasons, [("No caller found in the decomp", vec!["MTEST".to_string()]), (NO_ROW, vec!["ZZZZ".to_string()])]);
    }

    fn titles_of(section: &Section) -> Vec<String> {
        section.units.iter().map(|unit| unit.title.clone()).collect()
    }

    #[test]
    fn the_groups_come_in_order() {
        let catalog = catalog(&disc("catalog-groups"));
        let order: Vec<_> = catalog.groups.iter().map(|group| group.which).collect();
        let mut sorted = order.clone();
        sorted.sort();
        assert_eq!(order, sorted);
        assert!(!order.contains(&Which::Battle), "a group with no entries is left out");
    }

    #[test]
    fn the_tree_opens_by_key_and_a_filter_opens_the_way_to_its_match() {
        let catalog = catalog(&disc("catalog-tree"));

        let closed = catalog.lines("", &HashSet::new());
        assert!(closed.iter().all(|line| line.depth == 0), "every group starts shut");

        let mut open = HashSet::new();
        open.insert(closed.iter().find(|line| line.label == "Areas and maps").expect("areas").key.clone());
        let areas = catalog.lines("", &open);
        assert!(areas.iter().any(|line| line.depth == 1 && line.label == "Areas"));

        let found = catalog.lines("area 100", &HashSet::new());
        let labels: Vec<_> = found.iter().map(|line| (line.depth, line.label.as_str())).collect();
        assert!(labels.contains(&(0, "Areas and maps")) && labels.contains(&(2, "Area 100 (WORLD01)")), "{labels:?}");
        assert!(!labels.iter().any(|(_, label)| label.contains("Area 10 ")), "{labels:?}");

        let entries = catalog.lines("window", &HashSet::new());
        let leaves: Vec<_> = entries.iter().filter(|line| line.entry.is_some()).collect();
        assert_eq!(leaves.len(), 3, "the three window entries");
        assert!(leaves.iter().all(|line| line.archive.is_some()));
    }

    #[test]
    fn the_panel_mode_is_remembered_in_the_project_folder() {
        let disc = disc("catalog-mode");
        let project = Project::open(&disc.disc(), &disc.project()).expect("project opens");

        assert_eq!(Mode::read(&project.folder), Mode::Game);
        Mode::Disc.write(&project.folder);
        assert_eq!(Mode::read(&project.folder), Mode::Disc);
        Mode::Game.write(&project.folder);
        assert_eq!(Mode::read(&project.folder), Mode::Game);
    }
}
