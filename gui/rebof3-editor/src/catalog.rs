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
    use std::collections::BTreeSet;

    use super::*;
    use crate::fixture::{Disc, Entry};
    use crate::project::Project;

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
}
