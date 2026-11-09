//! Every retail character sprites decodes and encodes to its own bytes, every section's animations stitch

mod support;

use std::fs;
use std::path::{Path, PathBuf};

use rebof3_emi::replay::Vram;
use rebof3_emi::{Archive, lz77};
use rebof3_formats::sprite::{self, FIELD_PALETTE, Placement, Reader, SpriteSizes, Stage, compose, frame_bounds};
use rebof3_formats::{Format, classify};

/// The executable that holds `g_SpriteSizes`.
const EXECUTABLE: &str = "SLUS_004.22";

fn archives(folder: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(folder) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            archives(&path, out);
        } else if path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("emi")) {
            out.push(path);
        }
    }
}

/// Every character-sprites entry the classifier names, decoded: where it is and its container bytes.
fn containers(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut paths = Vec::new();
    archives(&root.join("BIN"), &mut paths);
    paths.sort();
    let mut found = Vec::new();
    for path in paths {
        let Ok(bytes) = fs::read(&path) else { continue };
        let Ok(archive) = Archive::parse(&bytes) else { continue };
        for (index, entry) in archive.entries().iter().enumerate() {
            let Ok(payload) = archive.payload(&bytes, index) else { continue };
            let container = match classify(entry, payload) {
                Format::CharacterSpritesLz77 => lz77::decode(payload).expect("a type-1 container decodes"),
                Format::CharacterSprites => payload.to_vec(),
                _ => continue,
            };
            found.push((format!("{}#{index}", path.strip_prefix(root).unwrap_or(&path).display()), container));
        }
    }
    found
}

#[test]
fn every_retail_character_sprites_round_trips_and_stitches() {
    let Some(root) = support::retail_disc() else {
        return;
    };
    let sizes = SpriteSizes::from_executable(&fs::read(root.join(EXECUTABLE)).expect("the executable reads"))
        .expect("g_SpriteSizes reads");

    let (mut entries, mut decoded, mut sections, mut failures) = (0, 0, 0, Vec::new());
    let (mut sheets, mut animations, mut placed, mut frames, mut stitched) = (0, 0, 0, 0, 0);
    let (mut outside, mut unresolved, mut imported, mut repacked) = (0, 0, 0, 0);
    // Per reader: entries, sections and frames drawn with the palette the reader loads.
    let mut proven = std::collections::BTreeMap::<&str, (usize, usize, usize)>::new();
    for (place, bytes) in containers(&root) {
        entries += 1;
        let Ok(sprites) = sprite::decode(&bytes) else { continue };
        decoded += 1;
        let reader = Reader::of(place.split('#').next().unwrap_or(&place));
        if reader.palette().is_some() {
            let counts = proven.entry(if reader == Reader::Battle { "battle" } else { "field" }).or_default();
            counts.0 += 1;
            counts.1 += sprites.sections.len();
            counts.2 +=
                sprites.sections.iter().flat_map(|section| &section.animations).map(|a| a.frames.len()).sum::<usize>();
        }
        match sprite::encode(&sprites) {
            Ok(encoded) if encoded == bytes => {}
            Ok(_) => failures.push(format!("{place}: encodes to other bytes")),
            Err(error) => failures.push(format!("{place}: {error}")),
        }

        for (number, section) in sprites.sections.iter().enumerate() {
            sections += 1;
            sheets += section.images.len();
            for image in &section.images {
                let fresh = sprite::Image::new(image.width, image.height, image.encoding, image.texels().to_vec())
                    .expect("same texels")
                    .encode();
                repacked += usize::from(fresh.len() <= image.encode().len());
            }
            animations += section.animations.len();
            let mut stages: Vec<Option<Stage>> = vec![None; section.images.len()];
            for (index, animation) in section.animations.iter().enumerate() {
                frames += animation.frames.len();
                let Some(sheet) = section.image_of(index) else { continue };
                placed += 1;
                let stage = stages[sheet].get_or_insert_with(|| {
                    let palette = (FIELD_PALETTE, &section.palettes[FIELD_PALETTE]);
                    Stage::new(Vram::new(), (sheet, &section.images[sheet]), palette, Placement::default())
                });
                for frame in &animation.frames {
                    let Some(bounds) = frame_bounds(frame, &sizes, false) else { continue };
                    let composite = compose(stage, frame, &sizes, false, bounds);
                    stitched += 1;
                    outside += composite.outside;
                    unresolved += composite.unresolved_colours();
                }
            }

            let sheet = match sprite::export_section(section, number, FIELD_PALETTE, &sizes, "sheet.png") {
                Ok(sheet) => sheet,
                Err(error) => {
                    failures.push(format!("{place} section {number}: export: {error}"));
                    continue;
                }
            };
            match sprite::import_section(section, &sizes, &sheet.json, &sheet.png).and_then(|section| section.encode())
            {
                Ok(encoded) if section.encode().is_ok_and(|stored| stored == encoded) => imported += 1,
                Ok(_) => failures.push(format!("{place} section {number}: the sheet imports to other bytes")),
                Err(error) => failures.push(format!("{place} section {number}: import: {error}")),
            }
        }
    }

    eprintln!(
        "character sprites: {decoded} of {entries} entries decode and encode ({sections} sections, {sheets} frame sheets, \
         {animations} animations, {placed} played by a record, {frames} frames); {stitched} frames stitched, \
         {outside} cell texels past their sheet, {unresolved} pixels past the palette; {imported} of {sections} \
         sheets import to the stored section; {repacked} of {sheets} sheets pack again into their stored bytes"
    );
    eprintln!("proven palette (entries, sections, frames): {proven:?}");
    assert!(failures.is_empty(), "{} failures: {:?}", failures.len(), failures.iter().take(8).collect::<Vec<_>>());
    assert!(decoded > 0 && imported == sections, "{decoded} sprites, {imported} of {sections} sheets");
    assert_eq!(
        proven.get("battle"),
        Some(&(38, 114, 18310)),
        "battle sprites drawn with the palette their reader loads"
    );
    assert_eq!(proven.get("field"), Some(&(19, 57, 14892)), "field sprites drawn with the palette their reader loads");
}
