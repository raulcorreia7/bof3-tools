//! Whole-archive type-3 gate: decode, re-encode and place every VRAM upload.
//!
//! The test skips when it cannot find the retail archives. Point `BOF3_EMI_ROOT`
//! at a directory of `.EMI` containers (searched recursively) to run it
//! anywhere; otherwise it walks up from the crate directory looking for the
//! checkout's `raw/disc/BIN`.
//!
//! `bof3 emi vram -o <page.png> <archive.EMI>` dumps one archive's page set as
//! a raw 16-bit PNG for a visual check.

use std::fs;
use std::path::{Path, PathBuf};

use psx_formats::vram::{Descriptor, Plane, Rectangle, UPLOAD_TYPE, decode, encode};
use psx_formats::vram::{RECTANGLE_BYTES, RECTANGLE_SIDE, VRAM_HEIGHT, VRAM_WIDTH};
use rebof3_emi::Archive;

mod support;

use support::{RETAIL_SUFFIX, collect_archives, first_mismatch, retail_root};

/// What the archive gate checked and what it found.
#[derive(Default)]
struct Report {
    entries: usize,
    rectangles: usize,
    dropped_rectangles: usize,
    dropped_nonzero_rectangles: usize,
    failures: Vec<String>,
}

#[test]
fn retail_vram_uploads_round_trip() {
    let Some(root) = retail_root() else {
        eprintln!(
            "skipping type-3 gate: no retail archives at <ancestor>/{RETAIL_SUFFIX} \
             (set BOF3_EMI_ROOT to a directory of .EMI containers)"
        );
        return;
    };

    let mut archives = Vec::new();
    collect_archives(&root, &mut archives);
    archives.sort();
    assert!(!archives.is_empty(), "no .EMI containers under {}", root.display());

    let mut report = Report::default();
    let mut pages = Plane::new();
    check_archives(&archives, &mut report, &mut pages);

    assert!(report.entries > 0, "no type-3 entry under {}", root.display());
    assert!(
        report.failures.is_empty(),
        "{}/{} type-3 entries round-trip and place; {} failures:\n{}",
        report.entries - report.failures.len(),
        report.entries,
        report.failures.len(),
        report.failures.join("\n")
    );
    assert_eq!(
        report.dropped_nonzero_rectangles, 0,
        "the frame buffer drops {} rectangles that hold live data",
        report.dropped_nonzero_rectangles
    );
    eprintln!(
        "type-3 gate: {}/{} entries round-trip byte-exact; {} rectangles, \
         {} outside the frame buffer",
        report.entries, report.entries, report.rectangles, report.dropped_rectangles
    );
    let out = support::out_dir("retail_vram_uploads_round_trip");
    support::write(&out, "vram-pages.png", pages.png());
}

fn check_archives(archives: &[PathBuf], report: &mut Report, pages: &mut Plane) {
    for path in archives {
        check_archive(path, report, pages);
    }
}

fn check_archive(path: &Path, report: &mut Report, page_set: &mut Plane) {
    let retail = fs::read(path).expect("read retail archive");
    let archive = Archive::open(path).expect("open retail archive");
    for (index, entry) in archive.entries().iter().enumerate() {
        if entry.file_type != UPLOAD_TYPE {
            continue;
        }
        let payload = payload_of(&retail, entry.offset, entry.size);
        let descriptor = Descriptor::from_load_argument(entry.ram_ptr);
        if let Err(reason) = check_upload(payload, descriptor, page_set, report) {
            report.failures.push(format!("{}#{index}: {reason}", path.display()));
        }
    }
}

/// Decodes one payload, checks it against the format, and places it.
fn check_upload(
    payload: &[u8],
    descriptor: Descriptor,
    page_set: &mut Plane,
    report: &mut Report,
) -> Result<(), String> {
    let rectangles = decode(payload, descriptor).map_err(|error| error.to_string())?;
    if let Some(offset) = first_mismatch(&encode(&rectangles), payload) {
        return Err(format!("re-encoded payload differs at 0x{offset:X}"));
    }
    report.entries += 1;
    report.rectangles += rectangles.len();

    let dropped = check_destinations(&rectangles, descriptor, report)?;
    let skipped = page_set.blit(&rectangles);
    if skipped != dropped {
        return Err(format!("blit skipped {skipped} rectangles, placement says {dropped}"));
    }

    check_placed_words(&rectangles, payload, page_set)
}

/// Checks each rectangle against the format's destination arithmetic and books
/// the rectangles the frame buffer cannot hold. Returns the dropped count.
fn check_destinations(rectangles: &[Rectangle], descriptor: Descriptor, report: &mut Report) -> Result<usize, String> {
    let column = descriptor.column as usize;
    let row = descriptor.row as usize;
    let per_row = descriptor.chunks_per_row as usize;
    let mut dropped = 0;
    for (index, rectangle) in rectangles.iter().enumerate() {
        let (x, y) = destination(column, row, per_row, index);
        if (rectangle.x, rectangle.y) != (x, y) {
            return Err(format!("rectangle {index} decodes to ({}, {}), not ({x}, {y})", rectangle.x, rectangle.y));
        }
        if x + RECTANGLE_SIDE <= VRAM_WIDTH && y + RECTANGLE_SIDE <= VRAM_HEIGHT {
            continue;
        }
        dropped += 1;
        report.dropped_rectangles += 1;
        if rectangle.words.iter().any(|word| *word != 0) {
            report.dropped_nonzero_rectangles += 1;
        }
    }

    Ok(dropped)
}

/// The format's destination for rectangle `index`: `column` rectangles into the
/// row, one rectangle row down every `per_row` rectangles.
fn destination(column: usize, row: usize, per_row: usize, index: usize) -> (usize, usize) {
    ((column + index % per_row) * RECTANGLE_SIDE, (row + index / per_row) * RECTANGLE_SIDE)
}

/// Checks that every rectangle the frame buffer holds reads back the payload's
/// words at its destination.
fn check_placed_words(rectangles: &[Rectangle], payload: &[u8], page_set: &Plane) -> Result<(), String> {
    for (index, rectangle) in rectangles.iter().enumerate() {
        if rectangle.x + RECTANGLE_SIDE > VRAM_WIDTH || rectangle.y + RECTANGLE_SIDE > VRAM_HEIGHT {
            continue;
        }
        for row in 0..RECTANGLE_SIDE {
            for word in 0..RECTANGLE_SIDE {
                let offset = index * RECTANGLE_BYTES + row * RECTANGLE_SIDE * 2 + word * 2;
                let expected = u16::from_le_bytes([payload[offset], payload[offset + 1]]);
                let actual = page_set.word(rectangle.x + word, rectangle.y + row);
                if actual != expected {
                    return Err(format!(
                        "rectangle {index} word ({word}, {row}) is {actual:#06x} in the frame \
                         buffer, {expected:#06x} in the payload"
                    ));
                }
            }
        }
    }

    Ok(())
}

/// The entry's payload bytes. `Archive::open` validated the range.
fn payload_of(retail: &[u8], offset: u64, size: u32) -> &[u8] {
    let start = offset as usize;
    &retail[start..start + size as usize]
}
