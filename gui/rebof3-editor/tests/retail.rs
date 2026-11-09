//! Round trips over a whole extracted disc: every edit path the editor offers must write back exactly the
//! bytes it read. Point `BOF3_DISC_DIR` at an extracted disc (the folder that holds `BIN/`); without it the
//! tests skip.

use std::path::PathBuf;

use psx_formats::gfx::vram;
use rebof3_assets::decode::texture_words;
use rebof3_editor::project::{ArchivePath, Project};
use rebof3_editor::schema::{self, Field};
use rebof3_emi::text::codec::{self, Encoding};
use rebof3_emi::text::discover::{self, Kind};
use rebof3_formats::Format;
use rebof3_formats::map::MapBundle;

fn disc() -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os("BOF3_DISC_DIR")?);
    path.join("BIN").is_dir().then_some(path)
}

fn scratch(name: &str) -> PathBuf {
    let folder = std::env::temp_dir().join(format!("bof3-edit-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    folder
}

#[test]
fn a_project_folder_inside_the_disc_is_refused() {
    let Some(disc) = disc() else { return eprintln!("skipping: set BOF3_DISC_DIR") };

    assert!(Project::open(&disc, &disc.join("edits")).is_err());
}

#[test]
fn an_edit_saves_reloads_and_exports_verified() {
    let Some(disc) = disc() else { return eprintln!("skipping: set BOF3_DISC_DIR") };
    let folder = scratch("export");
    let game = ArchivePath("BIN/ETC/GAME.EMI".into());

    let mut project = Project::open(&disc, &folder).expect("project opens");
    let loaded = project.load(&game).expect("GAME.EMI loads");
    let mut payload = loaded.original(0).to_vec();
    payload[0] ^= 0xFF;
    project.set(&loaded, 0, payload.clone());
    assert_eq!(project.save(), Ok(1));

    let mut reopened = Project::open(&disc, &folder).expect("project reopens");
    let loaded = reopened.load(&game).expect("GAME.EMI loads");
    assert_eq!(reopened.payload(&loaded, 0), payload.as_slice());

    let files = reopened.export().expect("export verifies");
    assert_eq!(files.len(), 1);
    let rebuilt = rebof3_emi::Archive::parse(&std::fs::read(&files[0]).expect("rebuilt file")).expect("parses");
    assert_eq!(rebuilt.entries().len(), loaded.entries().len());

    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn every_entry_round_trips_through_its_editor() {
    let Some(disc) = disc() else { return eprintln!("skipping: set BOF3_DISC_DIR") };
    let folder = scratch("round-trip");
    let mut project = Project::open(&disc, &folder).expect("project opens");

    let mut textures = (0, Vec::new());
    let mut strings = (0, Vec::new());
    let mut maps = (0, Vec::new());
    for path in project.archives.clone() {
        let loaded = project.load(&path).expect("archive loads");
        for index in 0..loaded.entries().len() {
            let payload = loaded.original(index);
            let load_arg = loaded.entries()[index].ram_ptr;

            if loaded.formats[index] == Format::VramUpload {
                textures.0 += 1;
                let exact = texture_words(load_arg, payload)
                    .ok()
                    .and_then(|words| vram::payload_from_grid(words.width, words.height, &words.words).ok())
                    .is_some_and(|bytes| bytes == payload);
                if !exact {
                    textures.1.push(format!("{}#{index}", path.0));
                }
            }

            if loaded.formats[index] == Format::AreaMapBundle {
                maps.0 += 1;
                if MapBundle::decode(payload).map(|bundle| bundle.encode()).ok().as_deref() != Some(payload) {
                    maps.1.push(format!("{}#{index}", path.0));
                }
            }

            let found = discover::bank_records(payload).unwrap_or_else(|| discover::strings(payload, Some(load_arg)));
            for string in found {
                strings.0 += 1;
                let encoding =
                    if matches!(string.kind, Kind::Field { .. }) { Encoding::Name } else { Encoding::Message };
                let raw = &payload[string.start..string.end];
                let text = codec::decode(encoding, raw);
                if codec::encode(encoding, &text).ok().as_deref() != Some(raw) {
                    strings.1.push(format!("{}#{index}@{:#X}", path.0, string.start));
                }
            }
        }
    }

    eprintln!("textures: {} of {} do not round-trip", textures.1.len(), textures.0);
    eprintln!(
        "strings: {} of {} do not round-trip; first: {:?}",
        strings.1.len(),
        strings.0,
        strings.1.iter().take(8).collect::<Vec<_>>()
    );
    eprintln!("maps: {} of {} do not round-trip", maps.1.len(), maps.0);
    assert!(maps.1.is_empty(), "maps: {:?}", maps.1.iter().take(8).collect::<Vec<_>>());
    assert!(textures.1.is_empty(), "textures: {:?}", textures.1.iter().take(8).collect::<Vec<_>>());
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn every_sheet_fits_its_entries_and_rewrites_their_bytes() {
    let Some(disc) = disc() else { return eprintln!("skipping: set BOF3_DISC_DIR") };
    let folder = scratch("sheets");
    let mut project = Project::open(&disc, &folder).expect("project opens");

    let mut sheets = 0;
    let mut problems = Vec::new();
    for path in project.archives.clone() {
        let loaded = project.load(&path).expect("archive loads");
        for index in 0..loaded.entries().len() {
            let payload = loaded.original(index);
            for sheet in schema::sheets_for(&path.0, index, loaded.formats[index]) {
                sheets += 1;
                if payload.len() < sheet.end(payload) {
                    problems.push(format!("{}#{index} {}: ends at {:#x}", path.0, sheet.name, payload.len()));
                    continue;
                }
                let mut copy = payload.to_vec();
                for row in 0..sheet.rows(payload) {
                    let record = &mut copy[sheet.range(row)];
                    for column in &sheet.columns {
                        if let Field::Name(_) = column.field {
                            let bytes = schema::name_bytes(&schema::name_text(record, column), column)
                                .expect("a stored name encodes again");
                            record[column.offset..column.offset + bytes.len()].copy_from_slice(&bytes);
                        } else {
                            let value = schema::read(record, column);
                            schema::write(record, column, value);
                        }
                    }
                }
                if copy != payload {
                    problems.push(format!("{}#{index} {}: a rewrite changes bytes", path.0, sheet.name));
                }
            }
        }
    }

    eprintln!("sheets: {} of {sheets} do not fit or round-trip", problems.len());
    assert!(problems.is_empty(), "{:?}", problems.iter().take(8).collect::<Vec<_>>());
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn sequences_play_from_the_project_archive() {
    let Some(disc) = disc() else { return eprintln!("skipping: set BOF3_DISC_DIR") };
    let folder = scratch("sequences");
    let mut project = Project::open(&disc, &folder).expect("project opens");

    let (mut played, mut silent) = (0, Vec::new());
    for path in project.archives.clone() {
        let loaded = project.load(&path).expect("archive loads");
        let Some(index) = loaded.formats.iter().position(|format| *format == Format::SepPackage) else { continue };
        let Ok(package) = psx_formats::sep::Package::parse(loaded.original(index)) else { continue };
        let Some(sequence) = package.sequences.iter().find(|sequence| sequence.has_notes()) else { continue };

        let archive = project.current_archive(&loaded).expect("archive bytes");
        let pcm = rebof3_assets::sound::sequence_pcm(&path.0, &archive, u32::from(sequence.seq_id), 2.0)
            .unwrap_or_else(|error| panic!("{}: {error}", path.0));
        played += 1;
        if pcm.samples.iter().all(|&sample| sample == 0) {
            silent.push(path.0.clone());
        }
        if played == 8 {
            break;
        }
    }

    let _ = std::fs::remove_dir_all(&folder);
    eprintln!("sequences: {} of {played} render silent", silent.len());
    assert!(played > 0 && silent.is_empty(), "{silent:?}");
}

#[test]
fn samples_and_tones_play_through_the_spu() {
    use psx_formats::vab::Bank;
    use rebof3_editor::preview::{Keys, Note, note_pcm, sample_pcm};

    let Some(disc) = disc() else { return eprintln!("skipping: set BOF3_DISC_DIR") };
    let folder = scratch("tones");
    let mut project = Project::open(&disc, &folder).expect("project opens");
    let path = ArchivePath("BIN/BATTLE/BATL_SE.EMI".into());
    let loaded = project.load(&path).expect("archive loads");

    let (mut samples, mut tones, mut problems) = (0, 0, Vec::new());
    for header in (0..loaded.entries().len()).filter(|&index| loaded.formats[index] == Format::VabHeader) {
        let body = rebof3_emi::entry::vab_body_index(loaded.entries(), header).expect("the header has a body");
        let (header_bytes, body_bytes) = (loaded.original(header), loaded.original(body));
        let bank = Bank::parse(header_bytes, body_bytes).expect("a retail bank parses");
        for vag in bank.vags.iter().filter(|vag| !vag.is_empty()) {
            match sample_pcm(header_bytes, body_bytes, vag.index) {
                Ok(pcm) if pcm.samples.iter().any(|&sample| sample != 0) => samples += 1,
                Ok(_) => problems.push(format!("bank {header} sample {}: silent", vag.index)),
                // The library declines a sample whose tones are all silent; it has nothing to play.
                Err(error) if error.to_string().contains("carries no audio") => {}
                Err(error) => problems.push(format!("bank {header} sample {}: {error}", vag.index)),
            }
        }
        for tone in bank.usable_tones().filter(|tone| tone.min_note <= tone.max_note) {
            let keys = Keys::Tone { block: tone.storage_block, slot: tone.tone_slot };
            let note = tone.center_note.clamp(tone.min_note, tone.max_note);
            match note_pcm(header_bytes, body_bytes, &Note { keys, note, hold: 0.25 }) {
                Ok(pcm) if pcm.channels == 2 && pcm.sample_rate == psx_formats::VAB_SAMPLE_RATE => tones += 1,
                Ok(pcm) => problems
                    .push(format!("bank {header} tone {keys:?}: {} Hz, {} channels", pcm.sample_rate, pcm.channels)),
                Err(error) => problems.push(format!("bank {header} tone {keys:?}: {error}")),
            }
        }
    }

    let _ = std::fs::remove_dir_all(&folder);
    eprintln!("{}: {samples} samples and {tones} tones play, {} problems", path.0, problems.len());
    assert!(samples > 0 && tones > 0 && problems.is_empty(), "{:?}", problems.iter().take(8).collect::<Vec<_>>());
}
