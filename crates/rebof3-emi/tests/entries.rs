//! The per-entry decoders, over a synthetic archive on disk.

mod support;

use std::fs;

use psx_formats::fixtures::{tiny_bank, tiny_sep};
use psx_formats::vab::{BODY_TYPE, HEADER_TYPE};
use rebof3_emi::entry::{palette, sep};
use rebof3_emi::fixtures::archive_at;

/// A CLUT block of one 4bpp row.
fn bank() -> Vec<u8> {
    (0..16u16).flat_map(u16::to_le_bytes).collect()
}

/// An archive holding a VAB header and body, a SEP package and a palette.
fn archive_bytes() -> Vec<u8> {
    let (header, body) = tiny_bank();
    archive_at(&[(HEADER_TYPE, 0x8010_0000, &header), (BODY_TYPE, 0x8010_0000, &body), (psx_formats::sep::TYPE, 0, &tiny_sep()), (0, 0x8003_3800, &bank())])
}

/// The archive on disk, and its path.
fn archive_on_disk(directory: &std::path::Path) -> std::path::PathBuf {
    let path = directory.join("fixture.EMI");
    fs::write(&path, archive_bytes()).unwrap();
    path
}

#[test]
fn the_entry_decoders_write_what_they_read() {
    let out = support::out_dir("the_entry_decoders_write_what_they_read");
    let fixtures = support::out_dir("the_entry_decoders_input");
    let path = archive_on_disk(fixtures.as_path());

    let palette = palette(&path, 3).unwrap();
    support::write(&out, "palette.json", palette.json());
    support::write(&out, "palette-strip.png", palette.png_strip());

    let package = sep(&path, 2).unwrap();
    support::write(&out, "sequence-package.bin", package.encode());
}
