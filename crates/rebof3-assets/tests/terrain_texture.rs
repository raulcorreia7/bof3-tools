//! The textured top-down view over the retail area maps: every map bundle is drawn from its own area's replayed VRAM (`rebof3_emi::replay::scenes::field`). Point `BOF3_DISC_DIR` at an extracted disc (the folder that holds `BIN/`); without it the test skips.

mod support;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use rebof3_emi::Archive;
use rebof3_emi::replay::{self, Vram, scenes};
use rebof3_formats::map::MapBundle;
use rebof3_formats::map::terrain::top_down;
use rebof3_formats::{Format, classify};

fn collect(root: &Path, folder: &Path, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(folder) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, out);
        } else if let Ok(relative) = path.strip_prefix(root) {
            out.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
}

fn read(root: &Path, path: &str) -> Option<(Vec<u8>, Archive)> {
    let bytes = fs::read(root.join(path)).ok()?;
    let archive = Archive::parse(&bytes).ok()?;
    Some((bytes, archive))
}

#[test]
fn retail_area_maps_draw_from_their_replayed_vram() {
    let Some(root) = support::retail_disc() else {
        return;
    };
    let mut paths = Vec::new();
    collect(&root, &root.join("BIN"), &mut paths);
    paths.sort();

    let mut archives: BTreeMap<String, (Vec<u8>, Archive)> = BTreeMap::new();
    let (mut bundles, mut textured, mut unwritten, mut pixels, mut clear) = (0, 0, 0, 0usize, 0usize);
    for path in paths.iter().filter(|path| scenes::area_id(path).is_some()) {
        let scene = scenes::field(path);
        for archive in scene.archives() {
            if !archives.contains_key(archive)
                && let Some(read) = read(&root, archive)
            {
                archives.insert(archive.to_string(), read);
            }
        }
        let vram = Vram::replay(&scene.steps, &mut |archive| {
            let (bytes, archive) = archives.get(archive)?;
            let payloads =
                (0..archive.entries().len()).map(|slot| archive.payload(bytes, slot).unwrap_or(&[])).collect();
            Some(replay::Archive { entries: archive.entries(), payloads })
        });
        let Some((bytes, archive)) = archives.get(path.as_str()) else { continue };
        for (slot, entry) in archive.entries().iter().enumerate() {
            let Ok(payload) = archive.payload(bytes, slot) else { continue };
            if classify(entry, payload) != Format::AreaMapBundle {
                continue;
            }
            let Ok(bundle) = MapBundle::decode(payload) else { continue };
            let view = top_down(&bundle, &vram);
            bundles += 1;
            textured += view.textured;
            unwritten += view.unwritten;
            let drawn: Vec<u16> = view.words.iter().flatten().copied().collect();
            pixels += drawn.len();
            clear += drawn.iter().filter(|&&word| word == 0).count();
        }
    }
    eprintln!(
        "{bundles} bundles; {textured} cells textured, {unwritten} with an unwritten page or CLUT row; \
         {clear} of {pixels} drawn pixels are the transparent colour"
    );
    assert_eq!(bundles, 200, "every map bundle decodes and draws");
    assert_eq!((textured, unwritten), (688_118, 0));
}
