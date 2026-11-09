//! The area view over the retail disc: every area reads, renders and takes each edit back to the bytes
//! it read. Point `BOF3_DISC_DIR` at an extracted disc; without it the tests skip.

use gpui::TestAppContext;
use rebof3_formats::Format;
use rebof3_formats::encounter::{ENCOUNTER_ENEMIES, EncounterBlock};
use rebof3_formats::field::AREAS;
use rebof3_formats::map::draw::TileField;

use super::model::{self, Area, BACKGROUND, Layers, View};
use super::{Cache, read_area};
use crate::app::Editor;
use crate::project::{ArchivePath, Project};
use crate::view::Tab;

/// The frame each area renders at in the coverage pass.
const FRAME: (usize, usize) = (160, 120);

/// What the coverage pass counts.
#[derive(Debug, Default, PartialEq)]
struct Coverage {
    areas: usize,
    textured: usize,
    rendered: usize,
    with_actors: usize,
    actors: usize,
    zone_rows: usize,
    with_battle: usize,
    with_encounters: usize,
    exit_cells: usize,
    round_trips: usize,
    gltf_round_trips: usize,
}

fn scratch(name: &str) -> std::path::PathBuf {
    let folder = std::env::temp_dir().join(format!("bof3-edit-area-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    folder
}

/// The first map bundle of an archive.
fn bundle_index(project: &mut Project, archive: &ArchivePath) -> Option<usize> {
    project.load(archive).ok()?.formats.iter().position(|format| *format == Format::AreaMapBundle)
}

/// Each edit the view offers, at the value the data already holds, writes the same bytes; a changed
/// value changes only its own bytes. Returns the round trips made.
fn round_trips(area: &Area, bundle: &[u8]) -> usize {
    let mut trips = 0;
    let mut same = |name: &str, rebuilt: Result<Vec<u8>, String>, original: &[u8]| {
        assert_eq!(
            rebuilt.unwrap_or_else(|error| panic!("{name}: {error}")),
            original,
            "{name} rebuilds byte for byte"
        );
        trips += 1;
    };

    let cell = area.quads.first().map_or(0, |quad| quad.cell);
    let corner = area.bundle.cells[cell].heights[1];
    same("corner", model::with_corner(bundle, cell, 1, corner), bundle);
    same("code", model::with_code(bundle, cell, area.bundle.codes[cell]), bundle);
    same("raise by 0", model::with_raised(bundle, cell, 0), bundle);
    if let Some(tile) = area.quads.iter().find_map(|quad| quad.tile) {
        for field in TileField::ALL {
            same("tile field", model::with_tile_field(bundle, tile.index, field, field.get(tile.descriptor)), bundle);
        }
        let shaded = model::with_tile_field(bundle, tile.index, TileField::Shade, 0).expect("a shade");
        let differing = shaded.iter().zip(bundle).filter(|(new, old)| new != old).count();
        assert!(differing <= 1, "a shade edit touches one byte of the word, not {differing}");
    }
    let raised = model::with_raised(bundle, cell, if corner < 0 { 1 } else { -1 }).expect("a step");
    assert_eq!(raised.iter().zip(bundle).filter(|(new, old)| new != old).count(), 4, "four corner bytes");

    if let Some((_, grid)) = &area.battle {
        let value = area.battle_value(cell).expect("a battle value");
        same("battle", model::with_battle(grid, cell, value), grid);
    }
    if let Some((_, program)) = &area.program {
        for (index, actor) in area.actor_list().iter().enumerate() {
            let placement = actor.placement;
            if usize::from(placement.x2 / 2) < area.width() && usize::from(placement.y2 / 2) < area.height() {
                same("position", model::with_actor_position(area, index, placement.x2, placement.y2), program);
            }
            if let Some(facing) =
                placement.facing.filter(|&facing| usize::from(facing) < rebof3_formats::field::FACINGS)
            {
                same("facing", model::with_actor_facing(area, index, facing), program);
            }
            if let Some(id) = placement.scene.filter(|_| actor.scene.is_some()) {
                same("scene", model::with_actor_scene(area, index, id), program);
            }
        }
    }
    if let Some((_, block)) = &area.encounters
        && let Ok(parsed) = EncounterBlock::parse(block)
    {
        for (row, encounter) in parsed.encounters.iter().enumerate() {
            for slot in 0..ENCOUNTER_ENEMIES {
                let enemy = encounter.enemies[slot];
                assert!(
                    enemy == model::EMPTY_SLOT || usize::from(enemy) < parsed.enemies.len(),
                    "every retail slot names one of the block's enemy records or is empty"
                );
                same("enemy", model::with_encounter(block, row, slot, enemy), block);
            }
            same("rate", model::with_encounter(block, row, ENCOUNTER_ENEMIES, encounter.appearance_rate), block);
        }
    }
    trips
}

#[test]
fn every_retail_area_reads_renders_and_round_trips() {
    let Some(disc) = crate::retail_support::retail_disc() else { return };
    let folder = scratch("coverage");
    let mut project = Project::open(&disc, &folder).expect("the disc opens");
    let archives: Vec<ArchivePath> = project
        .archives
        .iter()
        .filter(|path| rebof3_emi::replay::scenes::area_id(&path.0).is_some())
        .cloned()
        .collect();
    let mut coverage = Coverage::default();
    for archive in &archives {
        let Some(index) = bundle_index(&mut project, archive) else { continue };
        let mut cache = Cache::default();
        let area = read_area(&mut project, archive, index, &mut cache)
            .unwrap_or_else(|error| panic!("{}: {error}", archive.0));
        coverage.areas += 1;
        coverage.textured += usize::from(!area.vram.is_empty() && area.unwritten == 0);
        let actors = area.actors.as_ref().unwrap_or_else(|error| panic!("{}: {error}", archive.0));
        coverage.with_actors += usize::from(!actors.is_empty());
        coverage.actors += actors.len();
        coverage.zone_rows += area.zones.as_ref().map_or(0, Vec::len);
        coverage.with_battle += usize::from(area.battle.is_some());
        coverage.with_encounters += usize::from(area.encounter_block().is_some());
        coverage.exit_cells += area
            .quads
            .iter()
            .filter(|quad| quad.face == rebof3_formats::map::draw::Face::Top)
            .filter(|quad| rebof3_formats::map::codes::is_exit(area.bundle.codes[quad.cell]))
            .count();

        let layers = Layers { codes: true, ..Layers::default() };
        let camera = View::overview(&area).camera(&area);
        let scene = model::scene(&area, layers, &camera, None);
        let frame = rebof3_render::software::draw(&scene, &camera, FRAME.0, FRAME.1);
        let drawn = frame.rgba.chunks(4).filter(|pixel| pixel[..3] != BACKGROUND[..3]).count();
        coverage.rendered += usize::from(drawn > 0);

        let bundle = project.payload(project.loaded(archive).expect("loaded"), index).to_vec();
        coverage.round_trips += round_trips(&area, &bundle);

        let glb = model::export(&area, &archive.0).expect("a glb");
        let (rebuilt, program) =
            model::import(&area, &bundle, &glb).unwrap_or_else(|error| panic!("{}: {error}", archive.0));
        assert_eq!(rebuilt, bundle, "{}: the heights come back byte for byte", archive.0);
        assert!(program.is_none(), "{}: no actor moves", archive.0);
        coverage.gltf_round_trips += 1;
    }
    eprintln!("{coverage:?}");
    // Every setup stream walks; one places no actor, and the 7 kind C records have no position, so
    // 5,280 of the 5,287 records are actors.
    let expected = Coverage {
        areas: AREAS,
        textured: AREAS,
        rendered: AREAS,
        with_actors: AREAS - 1,
        actors: 5280,
        zone_rows: rebof3_formats::field::ZONE_ROWS,
        with_battle: AREAS,
        with_encounters: AREAS,
        exit_cells: 4894,
        round_trips: 32220,
        gltf_round_trips: AREAS,
    };
    assert_eq!(coverage, expected);
    let _ = std::fs::remove_dir_all(&folder);
}

/// The 3D tab of a retail area reads its field data and draws a frame.
#[gpui::test]
fn a_retail_area_draws_in_3d(cx: &mut TestAppContext) {
    let Some(disc) = crate::retail_support::retail_disc() else { return };
    let folder = scratch("view");
    let project = folder.clone();
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::new(Some(disc), Some(project), cx));
    editor.update(cx, |editor, _| {
        editor.select_archive(&ArchivePath("BIN/WORLD00/AREA001.EMI".into()));
        let index = editor
            .archive
            .as_ref()
            .expect("the area loads")
            .formats
            .iter()
            .position(|format| *format == Format::AreaMapBundle);
        editor.select_entry(index.expect("the area has a map bundle"));
        editor.tab = Tab::Area;
    });
    cx.update(|window, cx| window.draw(cx).clear());
    let (summary, drawn) = editor.update(cx, |editor, _| (editor.views.area.summary(), editor.views.area.drawn()));
    let summary = summary.expect("the area reads");
    assert!(summary.contains("textured from Field: AREA001, 0 quads") && summary.contains(" actors;"), "{summary}");
    assert!(drawn.is_some(), "a frame is drawn");

    // A code edit and an actor facing edit change their own bytes, and undo restores them.
    let (before, after, undone) = editor.update(cx, |editor, _| {
        let area = super::shown_area(editor).expect("the area");
        let cell = area.quads[0].cell;
        let program = area.program.as_ref().expect("a program").0;
        let bundle = editor.current().expect("an entry").index;
        let payloads = |editor: &Editor| {
            let entry = editor.current().expect("an entry");
            (entry.sibling(bundle).to_vec(), entry.sibling(program).to_vec())
        };
        let before = payloads(editor);
        editor.views.area.picked = Some(super::Target::Face { cell, face: area.quads[0].face });
        let code = area.bundle.codes[cell] ^ 1;
        assert_eq!(super::field_changed(editor, super::Part::Code, &code.to_string()), None);
        let actor =
            area.actor_list().iter().position(|actor| actor.placement.facing.is_some()).expect("a facing actor");
        let facing = (area.actor_list()[actor].placement.facing.unwrap() + 1) % rebof3_formats::field::FACINGS as u8;
        editor.views.area.picked = Some(super::Target::Actor(actor));
        assert_eq!(super::field_changed(editor, super::Part::Facing, &facing.to_string()), None);
        let after = payloads(editor);
        editor.undo();
        editor.undo();
        (before, after, payloads(editor))
    });
    let changed = |old: &[u8], new: &[u8]| old.iter().zip(new).filter(|(a, b)| a != b).count();
    assert_eq!(changed(&before.0, &after.0), 1, "one code byte");
    assert_eq!(changed(&before.1, &after.1), 1, "one facing byte");
    assert_eq!(undone, before, "undo restores both entries");
    let _ = std::fs::remove_dir_all(&folder);
}
