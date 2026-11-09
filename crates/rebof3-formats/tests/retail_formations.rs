//! Every random-encounter formation of every area places its enemies as `Area_PlaceEncounterEnemies`
//! does, finds each enemy's scene record, and draws each enemy's animation frames through the area's
//! replayed VRAM.
//!
//! Point `BOF3_DISC_DIR` at an extracted disc (the folder that holds `BIN/` and `SLUS_004.22`);
//! without it the test skips.

mod support;

use std::collections::BTreeMap;
use std::fs;

use rebof3_emi::Archive;
use rebof3_emi::replay::{self, Vram, scenes};
use rebof3_formats::formation::{self, Offsets};
use rebof3_formats::scene::SceneTable;
use rebof3_formats::script::program::Program;
use rebof3_formats::sprite::{SpriteSizes, Stage, compose, frame_bounds};
use rebof3_formats::{Format, classify};

const GAME: &str = "BIN/ETC/GAME.EMI";
const EXECUTABLE: &str = "SLUS_004.22";
/// Enemies whose first frame the test writes as a PNG, to look at.
const ARTIFACTS: usize = 24;

#[test]
fn retail_formations_place_and_draw() {
    let Some(root) = support::retail_disc() else { return };
    let read = |path: &str| {
        let bytes = fs::read(root.join(path)).unwrap_or_else(|error| panic!("{path}: {error}"));
        let archive = Archive::parse(&bytes).unwrap_or_else(|error| panic!("{path}: {error}"));
        (bytes, archive)
    };
    let (game_bytes, game) = read(GAME);
    let code = game.payload(&game_bytes, 0).expect("the code image");
    let offsets = Offsets::from_code(code, game.entries()[0].ram_ptr).expect("s_EncounterEnemyOffsets");
    let sizes = SpriteSizes::from_executable(&fs::read(root.join(EXECUTABLE)).expect("the executable")).expect("g_SpriteSizes");
    let (first_bytes, first) = read(scenes::FIRST);

    let mut paths: Vec<String> = rebof3_emi::archives_under(&root.join("BIN"))
        .into_iter()
        .filter_map(|path| Some(path.strip_prefix(&root).ok()?.to_string_lossy().replace('\\', "/")))
        .filter(|path| scenes::area_id(path).is_some())
        .collect();
    paths.sort();

    let (mut formations, mut enemies, mut found, mut streams, mut frames, mut drawn) = (0, 0, 0, 0, 0, 0);
    let (mut pixels, mut outside, mut unresolved) = (0usize, 0usize, 0usize);
    let out = support::out_dir("retail_formations");
    let mut written = 0;
    let mut archives = BTreeMap::new();
    archives.insert(scenes::FIRST.to_string(), (first_bytes, first));
    for path in &paths {
        let (bytes, archive) = read(path);
        let payload = |load: u32| {
            archive
                .entries()
                .iter()
                .enumerate()
                .find(|(slot, entry)| entry.ram_ptr == load && classify(entry, archive.payload(&bytes, *slot).unwrap_or(&[])) != Format::Reserve)
                .and_then(|(slot, _)| archive.payload(&bytes, slot).ok())
        };
        let Some(block) = payload(0x800E_4000) else { continue };
        let table = payload(0x800E_3800).and_then(|bytes| SceneTable::decode(bytes).ok());
        let program = payload(0x800D_3800).and_then(|bytes| Program::decode(bytes).ok());
        let scene = scenes::field(path);
        let vram = Vram::replay(&scene.steps, &mut |name| {
            let (bytes, archive) = if name == path { (&bytes, &archive) } else { archives.get(name).map(|(b, a)| (b, a))? };
            let payloads = (0..archive.entries().len()).map(|slot| archive.payload(bytes, slot).unwrap_or(&[])).collect();
            Some(replay::Archive { entries: archive.entries(), payloads })
        });

        for encounter in 0..rebof3_formats::encounter::ENCOUNTERS {
            let placed = formation::place(block, table.as_ref(), encounter, &offsets, 0).expect("places");
            if placed.is_empty() {
                continue;
            }
            formations += 1;
            for enemy in placed {
                enemies += 1;
                let Some(record) = enemy.scene else { continue };
                found += 1;
                let Some(animation) = program.as_ref().and_then(|program| formation::stream(program, &record, 0)) else {
                    continue;
                };
                streams += 1;
                let stage = Stage::resident(vram.clone(), formation::placement(&record));
                for (number, frame) in animation.frames.iter().enumerate() {
                    frames += 1;
                    let Some(bounds) = frame_bounds(frame, &sizes, false) else { continue };
                    let composite = compose(&stage, frame, &sizes, false, bounds);
                    if number == 0 && written < ARTIFACTS {
                        written += 1;
                        let rgba: Vec<u8> = composite
                            .pixels
                            .iter()
                            .flat_map(|pixel| match pixel {
                                Some(pixel) if pixel.texel.drawn() => rebof3_formats::clut::color_rgba(pixel.texel.word),
                                _ => [0; 4],
                            })
                            .collect();
                        let png = psx_formats::gfx::png::rgba_png(bounds.width() as u32, bounds.height() as u32, &rgba);
                        let name = format!("{}-{encounter}-{}.png", scenes::area_id(path).unwrap_or(0), enemy.name);
                        support::write(&out, &name, png);
                    }
                    let shown = composite.pixels.iter().flatten().filter(|pixel| pixel.texel.drawn()).count();
                    drawn += usize::from(shown > 0);
                    pixels += shown;
                    outside += composite.outside;
                    unresolved += composite.unresolved_colours();
                }
            }
        }
    }
    eprintln!(
        "formations: {formations}, enemies {enemies}, {found} with a scene record, {streams} with a stream; \
         {frames} frames, {drawn} draw a pixel ({pixels} pixels, {outside} cell texels past an upload, \
         {unresolved} pixels past a loaded palette)"
    );
    assert_eq!((formations, enemies, found, streams), (599, 1872, 1780, 1780), "formations, enemies, scene records, streams");
    assert_eq!((frames, drawn), (16322, 16322), "frames and frames that draw");
}
