//! The load replay over the retail archives: every default scene replays, every type-3 entry
//! lands in the scene that loads it, and the replayed VRAM agrees with the palette joins proven
//! earlier from the archives alone.
//!
//! Point `BOF3_DISC_DIR` at an extracted disc (the folder that holds `BIN/`); without it the test
//! skips.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use psx_formats::gfx::texture::{Clut, Depth};
use rebof3_emi::Archive;
use rebof3_emi::replay::{self, Vram, Writer, scenes};

/// One archive's bytes and table, read once.
struct Disc {
    root: PathBuf,
    archives: BTreeMap<String, (Vec<u8>, Archive)>,
}

impl Disc {
    fn open() -> Option<Self> {
        let root = PathBuf::from(std::env::var_os("BOF3_DISC_DIR")?);
        root.join("BIN").is_dir().then(|| Self { root, archives: BTreeMap::new() })
    }

    fn paths(&self) -> Vec<String> {
        let mut paths = Vec::new();
        collect(&self.root, &self.root.join("BIN"), &mut paths);
        paths.sort();
        paths
    }

    fn read(&mut self, path: &str) -> bool {
        if !self.archives.contains_key(path) {
            let Ok(bytes) = fs::read(self.root.join(path)) else { return false };
            let Ok(archive) = Archive::parse(&bytes) else { return false };
            self.archives.insert(path.to_string(), (bytes, archive));
        }
        true
    }

    fn replay(&mut self, scene: &scenes::Scene) -> Vram {
        for path in scene.archives() {
            self.read(path);
        }
        let archives = &self.archives;
        Vram::replay(&scene.steps, &mut |path| {
            let (bytes, archive) = archives.get(path)?;
            let payloads =
                (0..archive.entries().len()).map(|slot| archive.payload(bytes, slot).unwrap_or(&[])).collect();
            Some(replay::Archive { entries: archive.entries(), payloads })
        })
    }
}

fn collect(root: &Path, folder: &Path, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(folder) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, out);
        } else if path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("emi"))
            && let Ok(relative) = path.strip_prefix(root)
        {
            out.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
}

#[test]
fn retail_scenes_replay_and_place_every_upload() {
    let Some(mut disc) = Disc::open() else { return eprintln!("skipping: set BOF3_DISC_DIR") };
    let paths = disc.paths();
    let catalog = scenes::catalog(paths.iter().map(String::as_str));
    assert!(catalog.len() > 200, "{} scenes", catalog.len());

    // The type-3 entries a scene's own last archive wrote, counted over every scene that loads it last.
    let mut placed = std::collections::BTreeSet::new();
    let mut uploads = 0;
    for path in &paths {
        assert!(disc.read(path), "{path}");
        let archive = &disc.archives[path].1;
        uploads += archive.entries().iter().filter(|entry| entry.file_type == 3).count();
    }
    for scene in &catalog {
        let vram = disc.replay(scene);
        assert!(vram.missing().is_empty(), "{}: missing {:?}", scene.name, vram.missing());
        for region in vram.regions() {
            if let Some((archive, slot)) = region.writer.as_entry() {
                placed.insert((archive.to_string(), slot));
            }
        }
    }
    let unplaced: Vec<String> = paths
        .iter()
        .filter(|path| scenes::home(path).is_some())
        .flat_map(|path| {
            let archive = &disc.archives[path].1;
            archive
                .entries()
                .iter()
                .enumerate()
                .filter(|(_, entry)| entry.file_type == 3)
                .filter(|(slot, _)| !placed.contains(&(path.clone(), *slot)))
                .map(|(slot, _)| format!("{path} #{slot}"))
                .collect::<Vec<_>>()
        })
        .collect();
    let homeless: Vec<&String> = paths
        .iter()
        .filter(|path| scenes::home(path).is_none())
        .filter(|path| disc.archives[*path].1.entries().iter().any(|entry| entry.file_type == 3))
        .collect();
    eprintln!("{uploads} uploads; {} placed; unplaced {unplaced:?}; no scene {homeless:?}", placed.len());
    // FIRST's entry 12, (832, 256) four rectangles wide, covers all of entry 4 at (896, 256).
    assert_eq!(unplaced, ["BIN/ETC/FIRST.EMI #4"], "uploads no scene shows");
    // `SCENA17` and `ENDKANJI` have no default scene: their loaders are not read.
    assert_eq!(homeless, ["BIN/ETC/ENDKANJI.EMI", "BIN/SCENARIO/SCENA17.EMI"]);
}

/// The joins the archive-only rule proves (`texture::joins`) hold in the replayed VRAM too.
#[test]
fn retail_replay_agrees_with_the_proven_joins() {
    let Some(mut disc) = Disc::open() else { return eprintln!("skipping: set BOF3_DISC_DIR") };

    // AREA024: the literal site's 4-bit page (704, 256) is entry 7, and row (160, 483) is bank 8.
    let area = "BIN/WORLD00/AREA024.EMI";
    let vram = disc.replay(&scenes::field(area));
    assert_eq!(vram.writer(704, 256), Some(&Writer::entry(area, 7)));
    let clut = Clut::new(160, 483).expect("a CLUT");
    assert_eq!(vram.clut_writer(clut, Depth::Four), Some(&Writer::entry(area, 8)));

    // MAGIC218: row (0, 506) is the effect archive's bank 5, at 8 bits.
    let magic = "BIN/BMAGIC/MAGIC218.EMI";
    let scene = scenes::in_battle(scenes::battle(scenes::frontend(), scenes::BATTLE), magic).expect("an effect");
    let vram = disc.replay(&scene);
    let clut = Clut::new(0, 506).expect("a CLUT");
    assert_eq!(vram.clut_writer(clut, Depth::Eight), Some(&Writer::entry(magic, 5)));
}

/// A texture's known rows: depth bits and CLUT position.
type Rows = std::collections::BTreeSet<(u32, u16, u16)>;

/// What the rules join over every default scene, and how the ranked suggestion does on those
/// joins with the joins hidden.
#[test]
fn retail_joins_and_the_suggestion_scorer() {
    use psx_formats::gfx::vram::{Descriptor, word_grid};
    use rebof3_emi::map::MapBundle;
    use rebof3_emi::texture::{scene as joins, suggest};
    use rebof3_formats::{Format, classify};
    use std::collections::{BTreeMap, BTreeSet};

    let Some(mut disc) = Disc::open() else { return eprintln!("skipping: set BOF3_DISC_DIR") };
    let paths = disc.paths();
    // Known joins per texture: (depth bits, clut) pairs, from the texture's home scene.
    let mut known: BTreeMap<(String, usize), Rows> = BTreeMap::new();
    let mut joined = BTreeSet::new();
    for scene in scenes::catalog(paths.iter().map(String::as_str)) {
        let vram = disc.replay(&scene);
        let mut descriptors = Vec::new();
        for path in scene.archives() {
            let (bytes, archive) = &disc.archives[path];
            for (slot, entry) in archive.entries().iter().enumerate() {
                let payload = archive.payload(bytes, slot).unwrap_or(&[]);
                if classify(entry, payload) == Format::AreaMapBundle
                    && let Ok(bundle) = MapBundle::decode(payload)
                {
                    descriptors.extend(bundle.records);
                }
            }
        }
        let found = joins::scene_joins(&scene, &vram, &descriptors);
        for join in found {
            let Some((archive, slot)) = join.texture.as_entry() else { continue };
            joined.insert((archive.to_string(), slot));
            if scenes::home(archive).is_some_and(|home| home == scene) {
                known.entry((archive.to_string(), slot)).or_default().insert((
                    join.depth.bits(),
                    join.clut.x,
                    join.clut.y,
                ));
            }
        }
    }
    eprintln!("textures joined: {} of 593", joined.len());

    let (mut depth_hits, mut row_hits, mut top5_hits, mut four_bit_hits, mut eight_bit_hits, mut first_row_hits) =
        (0, 0, 0, 0, 0, 0);
    for ((archive, slot), truth) in &known {
        let (bytes, table) = &disc.archives[archive];
        let entry = &table.entries()[*slot];
        let payload = table.payload(bytes, *slot).expect("the payload");
        let grid = word_grid(payload, Descriptor::from_load_argument(entry.ram_ptr)).expect("the grid");
        let home = scenes::home(archive).expect("a home scene");
        let vram = disc.replay(&home);
        let suggestion = suggest::suggest(&vram, &grid.words, grid.width, grid.height, archive);
        let depths: BTreeSet<u32> = truth.iter().map(|(bits, _, _)| *bits).collect();
        let top = suggestion.depths[0].depth.map_or(16, |depth| depth.bits());
        depth_hits += usize::from(depths.contains(&top));
        four_bit_hits += usize::from(depths.contains(&4));
        eight_bit_hits += usize::from(depths.contains(&8));
        // Baseline: the first row the texture's own archive wrote, at the suggested depth.
        if let Some(best) = suggestion.best() {
            let mut rows = suggest::clut_candidates(&vram, &grid.words, grid.width, grid.height, best.depth, archive);
            rows.retain(|row| row.palette.as_entry().is_some_and(|(owner, _)| owner == archive));
            rows.sort_by_key(|row| (row.clut.y, row.clut.x));
            if let Some(first) = rows.first() {
                first_row_hits += usize::from(truth.contains(&(first.depth.bits(), first.clut.x, first.clut.y)));
            }
        }
        top5_hits += usize::from(
            suggestion.cluts.iter().take(5).any(|row| truth.contains(&(row.depth.bits(), row.clut.x, row.clut.y))),
        );
        if let Some(best) = suggestion.best() {
            row_hits += usize::from(truth.contains(&(best.depth.bits(), best.clut.x, best.clut.y)));
        }
    }
    let total = known.len();
    eprintln!(
        "suggestion over {total} joined textures: depth top-1 {depth_hits} ({:.0}%), always-4-bit {four_bit_hits}, \
         always-8-bit {eight_bit_hits}; depth and row top-1 {row_hits} ({:.0}%), top-5 {top5_hits}, first own row {first_row_hits}",
        100.0 * depth_hits as f64 / total as f64,
        100.0 * row_hits as f64 / total as f64
    );
    assert_eq!(joined.len(), 202, "the textures the rules join in some scene");
}
