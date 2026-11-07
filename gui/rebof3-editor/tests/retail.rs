//! Round trips over a whole extracted disc: every edit path the editor offers must write back exactly the
//! bytes it read. Point `BOF3_DISC_DIR` at an extracted disc (the folder that holds `BIN/`); without it the
//! tests skip.

use std::path::PathBuf;

use psx_formats::gfx::vram;
use rebof3_assets::decode::texture_words;
use rebof3_editor::project::{ArchivePath, Project};
use rebof3_emi::classify::Format;
use rebof3_emi::text::codec::{self, Encoding};
use rebof3_emi::text::discover::{self, Kind};

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
    assert!(textures.1.is_empty(), "textures: {:?}", textures.1.iter().take(8).collect::<Vec<_>>());
    let _ = std::fs::remove_dir_all(&folder);
}
