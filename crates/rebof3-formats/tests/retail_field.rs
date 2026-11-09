//! The field readers over the retail disc.

mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use rebof3_emi::replay::scenes::area_id;
use rebof3_formats::field::setup::{KINDS, walk};
use rebof3_formats::field::{AREAS, Executable, GAME_CODE, PROGRAM_ADDRESS, facing_scripts};

use support::{collect_archives, retail_disc};

/// `s_ScriptOpcodeLengths` (`emi/etc/game/00@0x801C8444`) inside `ETC/GAME.EMI`'s code image.
const LENGTHS_ADDRESS: u32 = 0x801C_8444;
/// The scene table's load address and the size of its records.
const SCENE_TABLE: u32 = 0x800E_3800;
const SCENE_RECORD: usize = 8;

/// Each entry's load address and payload.
fn entries(path: &PathBuf) -> Vec<(u32, Vec<u8>)> {
    let bytes = fs::read(path).expect("an archive");
    let archive = rebof3_emi::Archive::open(path).expect("an archive");
    archive
        .entries()
        .iter()
        .map(|entry| {
            let start = entry.offset as usize;
            (entry.ram_ptr, bytes[start..start + entry.size as usize].to_vec())
        })
        .collect()
}

#[test]
fn every_setup_stream_walks_and_names_its_scene_rows() {
    let Some(disc) = retail_disc() else { return };
    let exe_bytes = fs::read(disc.join("SLUS_004.22")).expect("the executable");
    let exe = Executable::new(&exe_bytes).expect("a PS-X EXE");
    let mut archives = Vec::new();
    collect_archives(&disc.join("BIN"), &mut archives);

    let game = archives.iter().find(|path| path.ends_with("ETC/GAME.EMI")).expect("ETC/GAME.EMI");
    let code = entries(game).into_iter().find(|(ram, _)| *ram == GAME_CODE).expect("the game code image").1;
    let at = (LENGTHS_ADDRESS - GAME_CODE) as usize;
    let lengths: Vec<usize> = KINDS.iter().map(|layout| layout.length).collect();
    let stored: Vec<usize> = code[at..at + KINDS.len()].iter().map(|&length| usize::from(length)).collect();
    assert_eq!(lengths, stored, "the record lengths are s_ScriptOpcodeLengths");
    let scripts = facing_scripts(&code).expect("D_801C839C");
    assert_eq!(
        scripts,
        [[4, 0], [3, 0], [2, 0], [1, 0], [0, 0], [1, 1], [2, 1], [3, 1]],
        "one script per facing, mirrored"
    );

    let mut kinds: BTreeMap<u8, usize> = BTreeMap::new();
    let (mut walked, mut unmatched, mut zones) = (0, 0, 0);
    for path in &archives {
        let Some(area) = path.to_str().and_then(|text| area_id(&text.replace('\\', "/"))) else { continue };
        let area = usize::from(area);
        assert!(area < AREAS);
        let entries = entries(path);
        let program = &entries.iter().find(|(ram, _)| *ram == PROGRAM_ADDRESS).expect("an area program").1;
        let load = (exe.area_load(area).expect("an AreaLoad") - PROGRAM_ADDRESS) as usize;
        let start = u32::from_le_bytes(program[load..load + 4].try_into().unwrap()) - PROGRAM_ADDRESS;
        let stream = walk(program, start as usize).unwrap_or_else(|error| panic!("area {area}: {error}"));
        walked += 1;

        let table = &entries.iter().find(|(ram, _)| *ram == SCENE_TABLE).expect("a scene table").1;
        let count = u32::from_le_bytes(table[..4].try_into().unwrap()) as usize;
        let ids: BTreeSet<u16> = (0..count)
            .map(|row| u16::from_le_bytes([table[4 + row * SCENE_RECORD], table[5 + row * SCENE_RECORD]]))
            .collect();
        for record in &stream.records {
            *kinds.entry(record.kind).or_default() += 1;
            if let Some(scene) = record.placement(program).and_then(|placement| placement.scene)
                && !ids.contains(&scene)
            {
                unmatched += 1;
            }
        }
        zones += exe.zones(area).expect("a zone list").len();
    }
    eprintln!("walked {walked}, kinds {kinds:?}, unmatched scene ids {unmatched}, zone rows {zones}");
    assert_eq!(walked, AREAS);
    let expected = [
        (0x0, 1687),
        (0x1, 2251),
        (0x2, 39),
        (0x3, 84),
        (0x4, 5),
        (0x5, 168),
        (0x6, 49),
        (0x7, 94),
        (0x8, 39),
        (0x9, 252),
        (0xA, 25),
        (0xB, 181),
        (0xC, 7),
        (0xE, 406),
    ];
    assert_eq!(kinds.into_iter().collect::<Vec<_>>(), expected);
    assert_eq!(zones, rebof3_formats::field::ZONE_ROWS, "the 200 lists tile the zone table");
    assert_eq!(unmatched, 1, "one kind 0 record names a scene id its area's table lacks");
}
