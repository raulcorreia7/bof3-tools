//! The game catalog over the retail disc: every entry of every archive is filed exactly once.
//! Point `BOF3_DISC_DIR` at an extracted disc; without it the test skips, and `BOF3_REQUIRE_RETAIL=1`
//! makes a skip a failure.

mod support;

use std::collections::BTreeSet;

use rebof3_editor::catalog::{Catalog, Which, scan};
use rebof3_editor::project::Project;

/// The retail disc: its EMI archives and the entries in them.
const ARCHIVES: usize = 880;
const ENTRIES: usize = 6344;
/// The entries of `MTEST.EMI` and `RTEST.EMI`, which no caller loads (`wiki: appendix/open-questions`).
const UNSORTED_ENTRIES: usize = 3;

/// The retail groups: entries and archives. An archive counts in each group that holds one of its entries.
const GROUPS: [(Which, usize, usize); 10] = [
    (Which::Characters, 881, 159),
    (Which::Enemies, 1545, 440),
    (Which::Areas, 2640, 200),
    (Which::Battle, 818, 158),
    (Which::Interface, 52, 15),
    (Which::Audio, 252, 83),
    (Which::Text, 44, 44),
    (Which::DataTables, 2, 2),
    (Which::Other, 107, 31),
    (Which::Unsorted, 3, 2),
];

#[test]
fn every_retail_entry_is_filed_once() {
    let Some(disc) = support::retail_disc() else { return };
    let folder = std::env::temp_dir().join(format!("bof3-edit-test-catalog-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    let project = Project::open(&disc, &folder).expect("project opens");
    assert_eq!(project.archives.len(), ARCHIVES);

    let scanned = scan(&disc, &project.archives).expect("every archive reads");
    assert_eq!(scanned.iter().map(|archive| archive.entries.len()).sum::<usize>(), ENTRIES);
    let catalog = Catalog::build(&scanned);

    let mut filed = BTreeSet::new();
    for (archive, index) in catalog.placements() {
        assert!(filed.insert((archive.clone(), index)), "{} entry {index} is filed twice", archive.0);
    }
    assert_eq!(filed.len(), ENTRIES);
    for archive in &scanned {
        for index in 0..archive.entries.len() {
            assert!(filed.contains(&(archive.path.clone(), index)), "{} entry {index} is not filed", archive.path.0);
        }
    }

    let unsorted =
        catalog.groups.iter().find(|group| group.which == Which::Unsorted).map_or(0, |group| group.entries());
    assert_eq!(unsorted, UNSORTED_ENTRIES);
    let counts: Vec<_> = catalog.groups.iter().map(|group| (group.which, group.entries(), group.archives())).collect();
    assert_eq!(counts, GROUPS);
    let _ = std::fs::remove_dir_all(&folder);
}
