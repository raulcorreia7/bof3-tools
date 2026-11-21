//! The original bytes of every asset: the catalog's hashes match the disc
//! slices, and every type with a codec comes back to its own bytes.
//!
//! The test skips when the disc image is not there. Point `BOF3_DISC_IMAGE` at
//! its cue sheet (`BOF3_REQUIRE_RETAIL=1` makes a skip a failure).

mod support;

use std::collections::BTreeMap;
use std::fs;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use psx_cd::Image;
use psx_cd::checksum::sha256_hex;
use rebof3_assets::extract::{self, Options};
use rebof3_assets::report::Report;
use rebof3_assets::types::Selection;
use rebof3_formats::vram::TextureImport;

/// The extracted tree and its catalog, extracted once for the whole file.
struct Extracted {
    /// The directory the tree is in. A static is never dropped, so the last test to finish removes it ([`Finished`]).
    /// A run that is killed first leaves it under the target directory, and the next run wipes it; it never lands in
    /// the system temp directory.
    directory: PathBuf,
    /// The tree's root.
    tree: PathBuf,
    /// What the run wrote.
    report: Report,
}

static EXTRACTED: OnceLock<Option<Extracted>> = OnceLock::new();

fn extracted() -> Option<&'static Extracted> {
    EXTRACTED
        .get_or_init(|| {
            let image = support::retail_image()?;
            let directory = support::out_dir("originals-extracted");
            let tree = directory.join("out");
            let options = Options { image, out: tree.clone(), selection: Selection::all(), write: true, jobs: 4 };
            let report = extract::run(&options).ok()?;
            Some(Extracted { directory, tree, report })
        })
        .as_ref()
}

/// The tests in this file that read the extracted tree.
const READERS: usize = 2;

/// How many of them have finished.
static FINISHED: AtomicUsize = AtomicUsize::new(0);

/// Held for the length of a test that reads the tree. The last one to drop removes the tree, so a full run leaves no
/// gigabytes behind. A filtered run that skips a reader leaves the tree for the next run to wipe.
struct Finished;

impl Drop for Finished {
    fn drop(&mut self) {
        if FINISHED.fetch_add(1, Ordering::SeqCst) + 1 == READERS
            && let Some(Some(extracted)) = EXTRACTED.get()
        {
            let _ = std::fs::remove_dir_all(&extracted.directory);
        }
    }
}

/// Every file under `directory`, as paths.
fn files_under(directory: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir(directory) else {
        return found;
    };
    for entry in entries.filter_map(std::result::Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            found.extend(files_under(&path));
        } else {
            found.push(path);
        }
    }
    found
}

/// The files under `root` whose path ends with `suffix`, in path order.
fn with_suffix(root: &Path, suffix: &str) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = files_under(root).into_iter().filter(|path| path.to_string_lossy().ends_with(suffix)).collect();
    found.sort();
    found
}

/// The file beside `path` with exactly this file name.
fn beside(path: &Path, name: &str) -> Option<PathBuf> {
    let directory = path.parent()?;
    files_under(directory).into_iter().find(|other| other.file_name().is_some_and(|file| file.to_string_lossy() == name))
}

#[test]
fn the_catalog_hashes_match_the_disc_slices() {
    let _finished = Finished;
    let Some(extracted) = extracted() else {
        return;
    };
    let Some(image) = support::retail_image() else {
        return;
    };
    let mut disc = Image::open(&image).expect("the disc opens");
    let entries = disc.entries().expect("the file table reads");
    let mut checked = 0;
    // The rows are in source order, so one disc file is read once for all its slots.
    let mut current: Option<(&str, Option<Vec<u8>>)> = None;
    for row in extracted.report.rows.iter().filter(|row| row.role == "raw" && row.slot.is_some()) {
        if current.as_ref().is_none_or(|(source, _)| *source != row.source) {
            let bytes = entries.iter().find(|entry| entry.path.to_string_lossy() == row.source).map(|entry| disc.read_file(entry).expect("the file reads"));
            current = Some((&row.source, bytes));
        }
        let Some((_, Some(bytes))) = &current else {
            continue;
        };
        let start = row.offset as usize;
        let end = start + row.size as usize;
        let slice = bytes.get(start..end).expect("the slice is inside the file");
        assert_eq!(sha256_hex(slice), row.sha256, "{} slot {:?}: the hash of the disc slice matches the catalog", row.source, row.slot);
        checked += 1;
    }
    eprintln!("checked {checked} hashes against the disc");
    assert!(checked > 0, "the catalog holds hashes to check");
}

#[test]
fn every_type_with_a_codec_comes_back_to_its_own_bytes() {
    let _finished = Finished;
    let Some(extracted) = extracted() else {
        return;
    };
    let tree = &extracted.tree;
    let mut round_trip: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    let mut record = |asset_type: &'static str, same: bool| {
        let counts = round_trip.entry(asset_type).or_default();
        counts.0 += 1;
        if same {
            counts.1 += 1;
        }
    };

    // A texture: its 16 bpp view reads back to the stored words.
    for path in with_suffix(tree, ".img") {
        if !path.to_string_lossy().contains("texture/") {
            continue;
        }
        let payload = fs::read(&path).unwrap();
        let slot = path.file_stem().unwrap().to_string_lossy().into_owned();
        let Some(view) = beside(&path, &format!("{slot}.png")) else { continue };
        let png = fs::read(view).unwrap();
        let same = match rebof3_formats::vram::import(&png) {
            Ok(TextureImport::Words(words)) => words == payload,
            Ok(TextureImport::Indexed(row)) => row.payload == payload,
            Err(_) => false,
        };
        record("texture", same);
    }

    // A palette: its JSON reads back to the stored words.
    for path in with_suffix(tree, ".clut") {
        if !path.to_string_lossy().contains("palette/") {
            continue;
        }
        let words = fs::read(&path).unwrap();
        let slot = path.file_stem().unwrap().to_string_lossy().into_owned();
        let Some(json) = beside(&path, &format!("{slot}.json")) else { continue };
        let document = fs::read(json).unwrap();
        let same = rebof3_formats::clut::import(&document).map(|back| back == words).unwrap_or(false);
        record("palette", same);
    }

    // A map bundle: its layout JSON reads back to the stored bytes.
    for path in with_suffix(tree, ".map") {
        if !path.to_string_lossy().contains("area-map/") {
            continue;
        }
        let payload = fs::read(&path).unwrap();
        let slot = path.file_stem().unwrap().to_string_lossy().into_owned();
        let Some(layout) = beside(&path, &format!("{slot}.layout.json")) else { continue };
        let document = fs::read(layout).unwrap();
        let same = rebof3_formats::map::layout::import(&document, &payload).map(|back| back == payload).unwrap_or(false);
        record("area-map", same);
    }

    // An encounter block: its JSON reads back to the stored bytes.
    for path in with_suffix(tree, ".emi0") {
        let name = path.to_string_lossy().into_owned();
        let asset_type = if name.contains("area-encounters/") {
            "area-encounters"
        } else if name.contains("area-message/") {
            "area-message"
        } else {
            continue;
        };
        let payload = fs::read(&path).unwrap();
        let slot = path.file_stem().unwrap().to_string_lossy().into_owned();
        let Some(json) = beside(&path, &format!("{slot}.json")) else { continue };
        let document = fs::read(json).unwrap();
        let same = if asset_type == "area-encounters" {
            rebof3_formats::encounter::open::import(&document, &payload).map(|back| back == payload).unwrap_or(false)
        } else {
            rebof3_formats::message::table::import(&document).map(|back| back == payload).unwrap_or(false)
        };
        record(asset_type, same);
    }

    // A cue table and a scene table: the codec rebuilds them.
    for path in with_suffix(tree, ".emi8") {
        let payload = fs::read(&path).unwrap();
        let same = rebof3_emi::cue::CueTable::decode(&payload).map(|table| table.encode() == payload).unwrap_or(false);
        record("audio-cues", same);
    }
    for path in with_suffix(tree, ".emi0") {
        let name = path.to_string_lossy().into_owned();
        if !name.contains("area-actor-defs/") {
            continue;
        }
        let payload = fs::read(&path).unwrap();
        let same = rebof3_formats::scene::SceneTable::decode(&payload).map(|table| table.encode() == payload).unwrap_or(false);
        record("area-actor-defs", same);
    }

    // A sequence: its MIDI reads back to the stored package.
    for path in with_suffix(tree, ".seq") {
        let stored = fs::read(&path).unwrap();
        let Ok(package) = psx_formats::sep::Package::parse(&stored) else { continue };
        let mut rebuilt = Vec::new();
        for (index, sequence) in package.sequences.iter().enumerate() {
            let Ok(midi) = package.to_midi(index) else { continue };
            if let Ok(back) = psx_formats::sep::Sequence::from_midi(&midi, sequence.seq_id)
                && back == *sequence
            {
                rebuilt.push(back);
            }
        }
        let same = (psx_formats::sep::Package { sequences: rebuilt }).encode() == stored;
        record("audio-sequence", same);
    }

    // A sound bank: its SoundFont reads back to a rebuilt bank, never the stored one. The import writes a fresh
    // header (default program volume and pan, ADSR fields searched from the SoundFont envelope) and re-encodes
    // every sample to ADPCM, so the bank bytes differ by design (`tests/soundfont.rs` gates the second round trip).
    // The kept content is the SoundFont: the rebuilt bank must export to the same zones, envelopes and samples.
    // The banks are independent and each takes a while, so they run on all cores.
    let banks = with_suffix(tree, ".sf2");
    let next = AtomicUsize::new(0);
    let workers = thread::available_parallelism().map_or(1, NonZeroUsize::get).min(banks.len()).max(1);
    let outcomes = thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut outcomes = Vec::new();
                    while let Some(path) = banks.get(next.fetch_add(1, Ordering::Relaxed)) {
                        let slot = path.file_stem().unwrap().to_string_lossy().into_owned();
                        let Some(header) = beside(path, &format!("{slot}.vh")) else { continue };
                        outcomes.push(sound_bank_round_trip(path, &header, &slot));
                    }
                    outcomes
                })
            })
            .collect();
        handles.into_iter().flat_map(|handle| handle.join().expect("a bank worker does not panic")).collect::<Vec<_>>()
    });
    let mut bank_bytes = 0;
    for (header_equal, same) in outcomes {
        bank_bytes += usize::from(header_equal);
        record("audio-bank (SoundFont content)", same);
    }
    eprintln!("audio-bank: {bank_bytes} rebuilt headers equal the stored header (expected 0: the import rebuilds it)");

    for (asset_type, (count, same)) in &round_trip {
        eprintln!("{asset_type}: {same} of {count} come back to their own bytes");
    }
    assert!(!round_trip.is_empty(), "the disc holds assets to round trip");
    // Every type comes back byte for byte; a sound bank comes back as the same SoundFont content.
    for (asset_type, (count, same)) in &round_trip {
        assert_eq!(same, count, "{asset_type} comes back to its own bytes");
    }
}

/// Whether a bank's rebuilt header equals the stored one, and whether its rebuilt SoundFont has the same content.
fn sound_bank_round_trip(soundfont_path: &Path, header_path: &Path, slot: &str) -> (bool, bool) {
    let stored = fs::read(header_path).unwrap();
    let soundfont = fs::read(soundfont_path).unwrap();
    let rebuilt = rebof3_formats::vab::soundfont::import(&soundfont);
    let header_equal = rebuilt.as_ref().is_ok_and(|(header, _)| *header == stored);
    let same = rebuilt
        .and_then(|(header, body)| rebof3_formats::vab::soundfont::export(&header, &body, slot))
        .ok()
        .and_then(|again| {
            let names_cleared = |bytes: &[u8]| {
                let mut font = psx_formats::sf2::read(bytes).ok()?;
                font.samples.iter_mut().for_each(|sample| sample.name.clear());
                Some(font)
            };
            Some(names_cleared(&again)? == names_cleared(&soundfont)?)
        })
        .unwrap_or(false);
    (header_equal, same)
}
