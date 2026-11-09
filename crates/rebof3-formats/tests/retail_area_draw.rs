//! The terrain draw over every retail map bundle.

mod support;

use std::collections::BTreeMap;
use std::fs;

use rebof3_formats::map::MapBundle;
use rebof3_formats::map::draw::{Face, quads};
use rebof3_formats::{Format, classify};

use support::{collect_archives, retail_root};

#[test]
fn every_drawn_cell_names_its_tiles() {
    let Some(root) = retail_root() else {
        return;
    };
    let mut archives = Vec::new();
    collect_archives(&root, &mut archives);
    archives.sort();

    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    let mut blends = [0_usize; 4];
    for path in archives {
        let Ok(bytes) = fs::read(&path) else { continue };
        let Ok(archive) = rebof3_emi::Archive::open(&path) else { continue };
        for entry in archive.entries() {
            let start = entry.offset as usize;
            let Some(payload) = bytes.get(start..start + entry.size as usize) else { continue };
            if classify(entry, payload) != Format::AreaMapBundle {
                continue;
            }
            let bundle = MapBundle::decode(payload).expect("a retail map bundle decodes");
            *counts.entry("bundles").or_default() += 1;
            for quad in quads(&bundle) {
                let face = match quad.face {
                    Face::Top => "tops",
                    Face::East => "east walls",
                    Face::South => "south walls",
                };
                *counts.entry(face).or_default() += 1;
                match quad.tile {
                    None => *counts.entry("quads past the descriptor table").or_default() += 1,
                    Some(tile) => {
                        if let Some(blend) = tile.blend {
                            blends[usize::from(blend.mode())] += 1;
                        }
                        if tile.descriptor == 0 {
                            *counts.entry("zero words").or_default() += 1;
                        }
                        if tile.shade != 0x80 {
                            *counts.entry("shaded quads").or_default() += 1;
                        }
                        if tile.sorted {
                            *counts.entry("sorted quads").or_default() += 1;
                        }
                        if tile.front {
                            *counts.entry("front quads").or_default() += 1;
                        }
                    }
                }
            }
        }
    }
    eprintln!("{counts:?} blends {blends:?}");
    let pinned = [
        ("bundles", 200),
        ("tops", 681_293),
        ("east walls", 39_924),
        ("south walls", 38_233),
        ("zero words", 3_614),
        ("shaded quads", 94_850),
        ("sorted quads", 28_424),
        ("front quads", 102_896),
    ];
    for (name, count) in pinned {
        assert_eq!(counts.get(name).copied().unwrap_or(0), count, "{name}");
    }
    assert!(!counts.contains_key("quads past the descriptor table"), "every wall names a descriptor in the table");
    assert_eq!(blends, [0; 4], "no retail terrain quad sets the semi-transparency bit");
}
