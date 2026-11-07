//! Export then import, byte for byte, over the texture kind of the retail disc.
//!
//! The test skips when no disc image is present. Point `BOF3_DISC_IMAGE` at its
//! cue sheet, or place the image in the checkout's `inputs/external`.

mod support;

use std::fs;
use std::path::{Path, PathBuf};

use rebof3_assets::decode;
use rebof3_assets::extract::{self, Options};
use rebof3_assets::import::{self, TextureImport};
use rebof3_assets::kind::{Kind, Selection};
use rebof3_assets::report::Row;
use rebof3_emi::game_tables::{PAYLOAD_START, TABLES};

/// Every file under `directory`, empty when the directory is missing.
fn files_under(directory: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files_under(&path, found);
        } else {
            found.push(path);
        }
    }
}

/// Whether `needle` appears in `bytes` as a run of little-endian words.
fn holds_run(bytes: &[u8], needle: &[u16]) -> bool {
    let needle: Vec<u8> = needle.iter().flat_map(|word| word.to_le_bytes()).collect();
    bytes.windows(needle.len()).any(|window| window == needle)
}

#[test]
fn retail_textures_round_trip_byte_exactly() {
    let Some(image) = support::retail_image() else {
        eprintln!("skipping texture round trip: no disc image (set BOF3_DISC_IMAGE)");
        return;
    };
    let dir = support::scratch_dir("round_trip-retail_textures_round_trip_byte_exactly");
    let options = Options {
        image,
        out: dir.to_path_buf(),
        selection: Selection::parse("texture,palette").unwrap(),
        write: true,
        jobs: 2,
    };
    let report = extract::run(&options).unwrap();
    assert!(report.failures.is_empty(), "{:?}", report.failures);

    let texture_root = dir.join("texture");
    let palette_root = dir.join("palette");
    let mut files = Vec::new();
    files_under(&texture_root, &mut files);
    let stored_count = files.iter().filter(|path| path.extension().is_some_and(|ext| ext == "img")).count();

    let (mut views, mut indexed, mut failures) = (0, 0, Vec::new());
    for path in &files {
        let Some(stem) = path.file_name().and_then(|name| name.to_str()).and_then(|name| name.strip_suffix(".png"))
        else {
            continue;
        };
        let (slot, is_row) = match stem.split_once(".clut-") {
            Some((slot, _)) => (slot, true),
            None => (stem, false),
        };
        let stored = fs::read(path.with_file_name(format!("{slot}.img"))).unwrap();
        match (import::texture(&fs::read(path).unwrap()), is_row) {
            (Ok(TextureImport::Words(payload)), false) => {
                views += 1;
                if payload != stored {
                    failures.push(format!(
                        "{}: the view does not return its {} stored bytes",
                        path.display(),
                        stored.len()
                    ));
                }
            }
            (Ok(TextureImport::Indexed(texture)), true) => {
                indexed += 1;
                if texture.payload != stored {
                    failures.push(format!("{}: the indexed row does not return its stored bytes", path.display()));
                }
                let relative = path.parent().unwrap().strip_prefix(&texture_root).unwrap();
                let mut banks = Vec::new();
                files_under(&palette_root.join(relative), &mut banks);
                let found = banks
                    .iter()
                    .filter_map(|bank| fs::read(bank).ok())
                    .any(|bytes| holds_run(&bytes, &texture.palette));
                if !found {
                    failures.push(format!("{}: its CLUT row is not in a palette bank of its archive", path.display()));
                }
            }
            (Ok(other), _) => failures.push(format!("{}: imported as {other:?}, not its kind", path.display())),
            (Err(error), _) => failures.push(format!("{}: {error}", path.display())),
        }
    }

    let mut palette_files = Vec::new();
    files_under(&palette_root, &mut palette_files);
    let mut palettes = 0;
    for json_path in palette_files.iter().filter(|path| path.extension().is_some_and(|ext| ext == "json")) {
        palettes += 1;
        let json = fs::read(json_path).unwrap();
        let stored = fs::read(json_path.with_extension("clut")).unwrap();
        if import::palette(&json).ok().as_deref() != Some(stored.as_slice()) {
            failures.push(format!(
                "{}: the JSON does not return its {} stored bytes",
                json_path.display(),
                stored.len()
            ));
        }
        if decode::palette_json(&stored).ok().as_deref() != Some(json.as_slice()) {
            failures.push(format!("{}: the export does not reproduce the JSON", json_path.display()));
        }
    }
    assert!(palettes > 0, "no palette JSON under {}", palette_root.display());
    let banks = palette_files.iter().filter(|path| path.extension().is_some_and(|ext| ext == "clut")).count();
    assert_eq!(palettes, banks, "every palette bank has a JSON");
    eprintln!("palette round trip: {palettes} banks return their stored bytes");

    assert!(views > 0, "no texture view under {}", texture_root.display());
    assert_eq!(views, stored_count, "every stored texture has one view");
    assert!(failures.is_empty(), "{} texture files do not round trip:\n{}", failures.len(), failures.join("\n"));
    eprintln!("texture round trip: {views} views and {indexed} indexed rows return their stored bytes");
}

#[test]
fn retail_terrains_round_trip_byte_exactly() {
    let Some(image) = support::retail_image() else {
        eprintln!("skipping terrain round trip: no disc image (set BOF3_DISC_IMAGE)");
        return;
    };
    let dir = support::scratch_dir("round_trip-retail_terrains_round_trip_byte_exactly");
    let options = Options {
        image,
        out: dir.to_path_buf(),
        selection: Selection::parse("area-map").unwrap(),
        write: true,
        jobs: 2,
    };
    let report = extract::run(&options).unwrap();
    assert!(report.failures.is_empty(), "{:?}", report.failures);

    let root = dir.join("area-map");
    let mut files = Vec::new();
    files_under(&root, &mut files);
    let (mut terrains, mut failures) = (0, Vec::new());
    for glb in files.iter().filter(|path| path.extension().is_some_and(|ext| ext == "glb")) {
        terrains += 1;
        let stored = fs::read(glb.with_extension("map")).unwrap();
        for mesh in [glb.clone(), glb.with_extension("obj")] {
            if import::terrain(&fs::read(&mesh).unwrap(), &stored).ok().as_deref() != Some(stored.as_slice()) {
                failures.push(format!(
                    "{}: the mesh does not return its {} stored bytes",
                    mesh.display(),
                    stored.len()
                ));
            }
        }
    }
    assert!(terrains > 0, "no terrain under {}", root.display());
    assert!(failures.is_empty(), "{} terrain meshes do not round trip:\n{}", failures.len(), failures.join("\n"));
    eprintln!("terrain round trip: {terrains} bundles return their stored bytes from the glTF and the OBJ");
}

/// ADPCM is lossy, so the proof is the samples: every VAG's decoded PCM encodes to a VAG that decodes to the
/// same PCM. The stored bytes are compared too, and the count of byte-identical VAGs is reported.
#[test]
fn retail_vags_round_trip_through_adpcm() {
    let Some(image) = support::retail_image() else {
        eprintln!("skipping VAG round trip: no disc image (set BOF3_DISC_IMAGE)");
        return;
    };
    let dir = support::scratch_dir("round_trip-retail_vags_round_trip_through_adpcm");
    let options = Options {
        image,
        out: dir.to_path_buf(),
        selection: Selection::parse("audio-bank").unwrap(),
        write: true,
        jobs: 2,
    };
    let report = extract::run(&options).unwrap();
    assert!(report.failures.is_empty(), "{:?}", report.failures);

    // A bank's header and body are two slots of one archive; the report pairs them by source and load argument.
    let is_bank = |row: &&Row| row.role == "raw" && row.kind == Kind::AudioBank;
    let (mut vags, mut identical, mut failures) = (0, 0, Vec::new());
    for header_row in report.rows.iter().filter(is_bank).filter(|row| row.file_type == Some(6)) {
        let body_row = report
            .rows
            .iter()
            .filter(is_bank)
            .find(|row| {
                row.file_type == Some(7) && row.source == header_row.source && row.load_arg == header_row.load_arg
            })
            .expect("every VAB header has its body");
        let header_path = dir.join(header_row.output.as_deref().unwrap());
        let header = fs::read(&header_path).unwrap();
        let body = fs::read(dir.join(body_row.output.as_deref().unwrap())).unwrap();
        let vab = match psx_formats::vab::Bank::parse(&header, &body) {
            Ok(vab) => vab,
            Err(error) => {
                failures.push(format!("{}: {error}", header_path.display()));
                continue;
            }
        };
        // VAG numbers are 1-based: slot 0 of the table is not a sample. An empty slot has no bytes to round-trip.
        for index in (1..vab.vags.len()).filter(|&index| !vab.vags[index].is_empty()) {
            vags += 1;
            let number = index as u32;
            let (Ok(stored), Ok(pcm)) = (vab.vag_bytes(&body, number), vab.decode_vag(&body, number)) else {
                failures.push(format!("{} VAG {index}: it does not decode", header_path.display()));
                continue;
            };
            let encoded = psx_formats::adpcm::encode_stream(&pcm);
            let decoded_again = psx_formats::adpcm::decode_stream(&encoded);
            if decoded_again.get(..pcm.len()) != Some(pcm.as_slice()) {
                failures.push(format!("{} VAG {index}: the encode changes its samples", header_path.display()));
            }
            let wav = decode::pcm_wav(&pcm).unwrap();
            if decode::vab_vag_wav(&header, &body, number).ok().as_deref() != Some(wav.as_slice()) {
                failures
                    .push(format!("{} VAG {index}: the bank export differs from its samples", header_path.display()));
            }
            if import::vag(&wav).unwrap() != encoded {
                failures.push(format!("{} VAG {index}: the WAVE path differs from the samples", header_path.display()));
            }
            identical += usize::from(encoded == stored);
        }
    }
    assert!(vags > 0, "no VAG under {}", dir.display());
    assert!(failures.is_empty(), "{} VAGs do not round trip:\n{}", failures.len(), failures.join("\n"));
    eprintln!("VAG round trip: {vags} VAGs keep their decoded samples; {identical} come back byte-identical");
}

#[test]
fn retail_tables_round_trip_byte_exactly() {
    let Some(image) = support::retail_image() else {
        eprintln!("skipping table round trip: no disc image (set BOF3_DISC_IMAGE)");
        return;
    };
    let dir = support::scratch_dir("round_trip-retail_tables_round_trip_byte_exactly");
    let options = Options {
        image,
        out: dir.to_path_buf(),
        selection: Selection::parse("data-table,code").unwrap(),
        write: true,
        jobs: 2,
    };
    let report = extract::run(&options).unwrap();
    assert!(report.failures.is_empty(), "{:?}", report.failures);

    // The eight tables are in entry 0 of ETC/GAME.EMI, which the code kind writes as its stored bytes.
    let entry = report
        .rows
        .iter()
        .find(|row| {
            row.kind == Kind::Code && row.role == "raw" && row.source == "BIN/ETC/GAME.EMI" && row.slot == Some(0)
        })
        .expect("ETC/GAME.EMI entry 0 is written as code");
    let payload = fs::read(dir.join(entry.output.as_deref().unwrap())).unwrap();
    for table in TABLES {
        let suffix = format!("/{}.json", table.name);
        let json_row = report
            .rows
            .iter()
            .find(|row| {
                row.kind == Kind::DataTable && row.output.as_deref().is_some_and(|path| path.ends_with(&suffix))
            })
            .unwrap_or_else(|| panic!("{} has a JSON file", table.name));
        let json = fs::read(dir.join(json_row.output.as_deref().unwrap())).unwrap();
        let start = table.archive_offset - PAYLOAD_START;
        let stored = &payload[start..start + table.count * table.record];
        assert_eq!(import::game_table_in_payload(table.name, &json, &payload).unwrap(), stored, "{}", table.name);
    }
    eprintln!("game table round trip: {} tables return their stored bytes from their JSON", TABLES.len());
}

#[test]
fn retail_encounters_round_trip_byte_exactly() {
    let Some(image) = support::retail_image() else {
        eprintln!("skipping encounter round trip: no disc image (set BOF3_DISC_IMAGE)");
        return;
    };
    let dir = support::scratch_dir("round_trip-retail_encounters_round_trip_byte_exactly");
    let options = Options {
        image,
        out: dir.to_path_buf(),
        selection: Selection::parse("area-encounters").unwrap(),
        write: true,
        jobs: 2,
    };
    let report = extract::run(&options).unwrap();
    assert!(report.failures.is_empty(), "{:?}", report.failures);

    let blocks: Vec<&Row> =
        report.rows.iter().filter(|row| row.kind == Kind::AreaEncounters && row.role == "raw").collect();
    assert!(!blocks.is_empty(), "no encounter block was written");
    let mut failures = Vec::new();
    for block in &blocks {
        let json_row = report
            .rows
            .iter()
            .find(|row| {
                row.kind == Kind::AreaEncounters
                    && row.role == "json"
                    && row.source == block.source
                    && row.slot == block.slot
            })
            .expect("every encounter block has its JSON");
        let stored = fs::read(dir.join(block.output.as_deref().unwrap())).unwrap();
        let json = fs::read(dir.join(json_row.output.as_deref().unwrap())).unwrap();
        if import::encounters(&json, &stored).ok().as_deref() != Some(stored.as_slice()) {
            failures.push(format!("{}: the JSON does not return its {} stored bytes", block.source, stored.len()));
        }
    }
    assert!(failures.is_empty(), "{} encounter blocks do not round trip:\n{}", failures.len(), failures.join("\n"));
    eprintln!("encounter round trip: {} blocks return their stored bytes from their JSON", blocks.len());
}

/// The layout of a map bundle keeps its byte codes, objects and height table. The stored bundle supplies the rest, so
/// the layout must rebuild every bundle's stored bytes, which also checks the bundle codec on all of them.
#[test]
fn retail_map_layouts_round_trip_byte_exactly() {
    let Some(image) = support::retail_image() else {
        eprintln!("skipping map layout round trip: no disc image (set BOF3_DISC_IMAGE)");
        return;
    };
    let dir = support::scratch_dir("round_trip-retail_map_layouts_round_trip_byte_exactly");
    let options = Options {
        image,
        out: dir.to_path_buf(),
        selection: Selection::parse("area-map").unwrap(),
        write: true,
        jobs: 2,
    };
    let report = extract::run(&options).unwrap();
    assert!(report.failures.is_empty(), "{:?}", report.failures);

    let bundles: Vec<&Row> = report.rows.iter().filter(|row| row.kind == Kind::AreaMap && row.role == "raw").collect();
    assert!(!bundles.is_empty(), "no map bundle was written");
    let mut failures = Vec::new();
    for bundle in &bundles {
        let layout_row = report
            .rows
            .iter()
            .find(|row| {
                row.kind == Kind::AreaMap
                    && row.role == "json"
                    && row.source == bundle.source
                    && row.slot == bundle.slot
                    && row.output.as_deref().is_some_and(|path| path.ends_with(".layout.json"))
            })
            .expect("every map bundle has its layout JSON");
        let stored = fs::read(dir.join(bundle.output.as_deref().unwrap())).unwrap();
        let json = fs::read(dir.join(layout_row.output.as_deref().unwrap())).unwrap();
        if import::map_layout(&json, &stored).ok().as_deref() != Some(stored.as_slice()) {
            failures.push(format!("{}: the layout does not return its {} stored bytes", bundle.source, stored.len()));
        }
    }
    assert!(failures.is_empty(), "{} map layouts do not round trip:\n{}", failures.len(), failures.join("\n"));
    eprintln!("map layout round trip: {} bundles return their stored bytes from their layout", bundles.len());
}

/// Every EMI container on the disc rebuilds from its own entries to the same bytes. Replacing one entry of the
/// first container changes that entry and no other.
#[test]
fn retail_emi_containers_rebuild_byte_exactly() {
    let Some(image_path) = support::retail_image() else {
        eprintln!("skipping EMI rebuild: no disc image (set BOF3_DISC_IMAGE)");
        return;
    };
    let mut image = psx_cd::Image::open(&image_path).unwrap();
    let entries = image.entries().unwrap();
    let containers: Vec<&psx_cd::Entry> = entries
        .iter()
        .filter(|entry| {
            !entry.is_directory && entry.path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("EMI"))
        })
        .collect();
    assert!(!containers.is_empty(), "no EMI container on the disc");

    let mut failures = Vec::new();
    let mut first = None;
    for entry in &containers {
        let bytes = image.read_file(entry).unwrap();
        if import::emi(&bytes, &[]).ok().as_deref() != Some(bytes.as_slice()) {
            failures.push(format!("{}: the rebuild differs from the container", entry.path.display()));
        }
        first.get_or_insert(bytes);
    }

    let bytes = first.expect("at least one container");
    let archive = rebof3_emi::Archive::parse(&bytes).unwrap();
    let count = archive.entries().len();
    assert!(count > 0, "the first container has no entries");
    let mut replacement = archive.payload(&bytes, 0).unwrap().to_vec();
    replacement.reverse();
    let edited = import::emi(&bytes, &[(0, &replacement)]).unwrap();
    let rebuilt = rebof3_emi::Archive::parse(&edited).unwrap();
    assert_eq!(rebuilt.entries().len(), count);
    assert_eq!(rebuilt.payload(&edited, 0).unwrap(), replacement.as_slice());
    for index in 1..count {
        assert_eq!(rebuilt.payload(&edited, index).unwrap(), archive.payload(&bytes, index).unwrap(), "entry {index}");
    }

    assert!(failures.is_empty(), "{} containers do not rebuild:\n{}", failures.len(), failures.join("\n"));
    eprintln!(
        "EMI rebuild: {} containers rebuild byte-exactly; a replaced entry changes only itself",
        containers.len()
    );
}

/// Every sequence package on the disc comes back from its MIDI files: each sequence reads back from the MIDI
/// that `to_midi` writes, and the packages encode to their stored bytes.
#[test]
fn retail_sequences_round_trip_through_midi() {
    let Some(image) = support::retail_image() else {
        eprintln!("skipping sequence round trip: no disc image (set BOF3_DISC_IMAGE)");
        return;
    };
    let dir = support::scratch_dir("round_trip-retail_sequences_round_trip_through_midi");
    let options = Options {
        image,
        out: dir.to_path_buf(),
        selection: Selection::parse("audio-sequence").unwrap(),
        write: true,
        jobs: 2,
    };
    let report = extract::run(&options).unwrap();
    assert!(report.failures.is_empty(), "{:?}", report.failures);

    let packages: Vec<&Row> =
        report.rows.iter().filter(|row| row.kind == Kind::AudioSequence && row.role == "raw").collect();
    assert!(!packages.is_empty(), "no sequence package was written");
    let (mut sequences, mut failures) = (0, Vec::new());
    for package_row in &packages {
        let stored = fs::read(dir.join(package_row.output.as_deref().unwrap())).unwrap();
        let package = match psx_formats::sep::Package::parse(&stored) {
            Ok(package) => package,
            Err(error) => {
                failures.push(format!("{}: {error}", package_row.source));
                continue;
            }
        };
        let mut rebuilt = Vec::new();
        for (index, sequence) in package.sequences.iter().enumerate() {
            sequences += 1;
            let midi = match package.to_midi(index) {
                Ok(midi) => midi,
                Err(error) => {
                    failures.push(format!("{} sequence {index}: {error}", package_row.source));
                    continue;
                }
            };
            match psx_formats::sep::Sequence::from_midi(&midi, sequence.seq_id) {
                Ok(back) if back == *sequence => rebuilt.push(back),
                Ok(_) => {
                    failures.push(format!("{} sequence {index}: the MIDI reads back differently", package_row.source))
                }
                Err(error) => failures.push(format!("{} sequence {index}: {error}", package_row.source)),
            }
        }
        if (psx_formats::sep::Package { sequences: rebuilt }).encode() != stored {
            failures.push(format!(
                "{}: the package does not encode to its {} stored bytes",
                package_row.source,
                stored.len()
            ));
        }
    }
    assert!(failures.is_empty(), "{} sequences do not round trip:\n{}", failures.len(), failures.join("\n"));
    eprintln!(
        "sequence round trip: {} packages, {sequences} sequences return their stored bytes from MIDI",
        packages.len()
    );
}
