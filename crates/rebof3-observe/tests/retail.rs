//! The new-game route on the retail disc: what the game's own draws prove about its textures, and
//! how that compares with the draw rules. Needs `BOF3_DISC_IMAGE`; skips without it.

mod support;

use rebof3_observe::compare::{Verdict, against_rules};
use rebof3_observe::{Route, disc_sha, run};

/// EMI files the route reads.
const FILES_READ: usize = 14;
/// Entries whose depth a draw proves.
const ENTRIES_OBSERVED: usize = 14;
/// Distinct depth and CLUT rows the draws use over those entries.
const WAYS_DRAWN: usize = 53;
/// Of the indexed ways, those whose CLUT row an entry of a file read holds.
const ROWS_NAMED: usize = 41;
/// Rule rows that a draw also names, and of those the ones the draws agree with.
const RULE_ROWS: usize = 14;
const RULE_ROWS_AGREEING: usize = 10;

#[test]
fn the_new_game_route_proves_texture_depths_and_rows() {
    let Some(image) = support::retail_image() else { return };
    let route = Route::built_in("new-game").expect("the built-in route parses");
    let sha = disc_sha(&image).expect("the disc image hashes");
    let observation = run(&image, &route, &sha, |_| true).expect("the route runs without a halt");
    eprintln!(
        "{} frames in {:.1} s: {} uploads, {} from an entry, {} textured draws",
        observation.frames,
        observation.seconds,
        observation.uploads,
        observation.attributed_uploads,
        observation.textured_draws
    );

    assert_eq!(observation.files.len(), FILES_READ, "files read: {:#?}", observation.files);
    // The first battle is the boss battle `BOSS/BOSS001`, which runs its own overlay. Only the
    // battle overlay 15 streams `BATL_DRA` (`Battle_LoadDraArchive`), so this route never loads it.
    assert!(observation.files.iter().all(|file| !file.ends_with("BATL_DRA.EMI")));
    assert!(observation.files.iter().any(|file| file == "BIN/BOSS/BOSS001.EMI"));

    let coverage = observation.coverage();
    assert_eq!(coverage.values().sum::<usize>(), ENTRIES_OBSERVED, "entries by archive: {coverage:#?}");
    assert_eq!(observation.textures.len(), WAYS_DRAWN);
    let named = observation.textures.iter().filter(|texture| texture.palette.is_some()).count();
    assert_eq!(named, ROWS_NAMED);

    // The area's terrain sheet: drawn most at 8 bits with CLUT row (0, 484), which the area's own
    // arena bank (slot 8) holds.
    let terrain = observation.of("BIN/WORLD00/AREA024.EMI", 7).next().expect("the terrain sheet is drawn");
    assert_eq!((terrain.bits, terrain.clut), (8, Some([0, 484])));
    let palette = terrain.palette.as_ref().expect("the row is named");
    assert_eq!((palette.archive.as_str(), palette.slot, palette.via.as_str()), ("BIN/WORLD00/AREA024.EMI", 8, "arena"));
    // The battle sound-effect sheet borrows the boss archive's bank: a row another archive loads.
    let effect = observation.of("BIN/BATTLE/BATL_SE.EMI", 3).next().expect("the battle effect is drawn");
    assert_eq!((effect.bits, effect.clut), (4, Some([0, 507])));
    let palette = effect.palette.as_ref().expect("the row is named");
    assert_eq!((palette.archive.as_str(), palette.slot), ("BIN/BOSS/BOSS001.EMI", 3));

    let comparisons = against_rules(&observation, &image).expect("the archives read");
    for comparison in comparisons.iter().filter(|comparison| comparison.verdict != Verdict::Agrees) {
        eprintln!(
            "disagrees: {} #{} rule {}-bit CLUT {},{} ({}): {:?}",
            comparison.archive,
            comparison.slot,
            comparison.depth.bits(),
            comparison.clut.x,
            comparison.clut.y,
            comparison.rule,
            comparison.verdict
        );
    }
    assert_eq!(comparisons.len(), RULE_ROWS);
    let agreeing = comparisons.iter().filter(|comparison| comparison.verdict == Verdict::Agrees).count();
    assert_eq!(agreeing, RULE_ROWS_AGREEING);
    // Every rule's depth agrees; the four that do not agree are the terrain rule's rows 485 and 487 of
    // the area's sheet, which the draws of page (448, 256) use as rows 486 and 488.
    assert!(comparisons.iter().all(|comparison| !matches!(comparison.verdict, Verdict::OtherDepth(_))));
}
