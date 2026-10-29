//! The playable characters of `ETC/START.EMI` entry 8, both ways. The retail check reads the owner's disc and skips
//! when no disc image is present (set `BOF3_DISC_IMAGE`).

mod support;

use std::path::{Path, PathBuf};

use psx_cd::Image;
use rebof3_assets::import;
use rebof3_assets::tables;
use rebof3_emi::Archive;
use rebof3_emi::characters::{self, CHARACTERS};

fn retail_cue() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("BOF3_DISC_IMAGE") {
        return Some(PathBuf::from(path));
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .find_map(|directory| psx_cd::discover_image(&directory.join("inputs/external")).ok())
}

#[test]
fn the_retail_characters_have_their_names_and_round_trip() {
    let Some(cue) = retail_cue() else {
        eprintln!("skipping the characters: no disc image (set BOF3_DISC_IMAGE)");
        return;
    };
    let mut image = Image::open(&cue).expect("the disc opens");
    let file = image
        .entries()
        .expect("the file table reads")
        .into_iter()
        .find(|file| file.path.display().to_string() == "BIN/ETC/START.EMI")
        .expect("START.EMI is on the disc");
    let container = image.read_file(&file).expect("the archive reads");
    let archive = Archive::parse(&container).expect("the archive parses");
    let payload = archive.payload(&container, 8).expect("entry 8 is in the archive");

    let names: Vec<String> =
        characters::parse(payload).expect("the characters read").into_iter().map(|c| c.name).collect();
    assert_eq!(names, ["Ryu", "Nina", "Garr", "Teepo", "Rei", "Momo", "Peco", "Whelp"]);
    assert_eq!(names.len(), CHARACTERS);

    let json = tables::characters(payload).expect("the characters export");
    assert!(import::characters(&json, payload).expect("the characters import") == payload);
    let out = support::out_dir("the_retail_characters_have_their_names_and_round_trip");
    support::write(&out, "characters.json", &json);
    eprintln!("{} characters: names and records round trip to the stored entry", names.len());
}
