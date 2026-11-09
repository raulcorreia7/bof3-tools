//! The character sprites' palette tables, both ways, on the owner's disc. The retail check skips when no disc image is
//! present (set `BOF3_DISC_IMAGE`).

mod support;

use rebof3_assets::coverage;

#[test]
fn every_retail_character_sprites_palette_table_round_trips() {
    let Some(cue) = support::retail_image() else {
        return;
    };
    let palettes = coverage::run(&cue).expect("the disc is read").palettes;
    assert_eq!(palettes.entries, 141, "the disc's character sprites in the palette layout");
    assert_eq!(palettes.both_entries, palettes.entries, "every palette table exports and imports to its own bytes");
    assert_eq!(palettes.both_bytes, 44_948, "the bytes of the palette tables");
    let out = support::out_dir("every_retail_character_sprites_palette_table_round_trips");
    support::write(
        &out,
        "palette-coverage.json",
        format!(
            "{{\"entries\":{},\"both_entries\":{},\"both_bytes\":{}}}",
            palettes.entries, palettes.both_entries, palettes.both_bytes
        ),
    );
}
