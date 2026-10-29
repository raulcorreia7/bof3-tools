//! The sprite banks' palette tables, both ways, on the owner's disc. The retail check skips when no disc image is
//! present (set `BOF3_DISC_IMAGE`).

mod support;

use std::path::{Path, PathBuf};

use rebof3_assets::coverage;

fn retail_cue() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("BOF3_DISC_IMAGE") {
        return Some(PathBuf::from(path));
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .find_map(|directory| psx_cd::discover_image(&directory.join("inputs/external")).ok())
}

#[test]
fn every_retail_sprite_bank_palette_table_round_trips() {
    let Some(cue) = retail_cue() else {
        eprintln!("skipping the sprite palettes: no disc image (set BOF3_DISC_IMAGE)");
        return;
    };
    let palettes = coverage::run(&cue).expect("the disc is read").palettes;
    assert_eq!(palettes.entries, 141, "the disc's sprite banks in the palette layout");
    assert_eq!(palettes.both_entries, palettes.entries, "every palette table exports and imports to its own bytes");
    assert_eq!(palettes.both_bytes, 44_948, "the bytes of the palette tables");
    let out = support::out_dir("every_retail_sprite_bank_palette_table_round_trips");
    support::write(
        &out,
        "palette-coverage.json",
        format!(
            "{{\"entries\":{},\"both_entries\":{},\"both_bytes\":{}}}",
            palettes.entries, palettes.both_entries, palettes.both_bytes
        ),
    );
}
