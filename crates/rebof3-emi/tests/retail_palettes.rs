//! Whole-archive CLUT gate: every palette payload decodes and re-encodes to the
//! same bytes.
//!
//! The gate covers every type-0 entry whose load argument lands in the CLUT band
//! (`wiki: formats/graphics#palette`), whichever archive family it comes
//! from. Like `tests/retail_repack.rs`, the gate skips when it cannot find the retail
//! archives; `support::retail_root` owns the lookup and honours `BOF3_DISC_DIR` (the archives are `BIN/` below it).

mod support;

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use psx_formats::{Error, clut};
use rebof3_emi::{Archive, Entry};

/// Type-0 entries load here as CLUT blocks (`wiki: formats/graphics#palette`).
const CLUT_BAND: std::ops::RangeInclusive<u32> = 0x8003_3800..=0x8003_6e00;
/// Palette entries the asset census counts in that band (575 entries,
/// 1,119,296 bytes); the band also holds 45 other type-0 payloads.
const CENSUS_PALETTES: usize = 575;
/// Payload size of the palette shape the census documents in `wiki: formats/emi`.
const SAMPLE_PAYLOAD: u32 = 512;
/// Swatch edge of the rendered strip, pinned against `clut::Palette::png_strip`.
const SWATCH_PIXELS: u32 = 16;

fn read_payload(archive_path: &Path, entry: &Entry) -> Vec<u8> {
    let mut source = fs::File::open(archive_path).expect("open retail archive");
    source.seek(SeekFrom::Start(entry.offset)).expect("seek to entry");
    let mut payload = vec![0; entry.size as usize];
    source.read_exact(&mut payload).expect("read entry payload");
    payload
}

/// Decodes one payload and checks what the codec promises for it: the encoded
/// bank is the payload again, and the RGBA view loses nothing but STP.
fn round_trip(where_: &str, payload: &[u8]) -> Result<clut::Palette, String> {
    let palette = clut::Palette::decode(payload).map_err(|error| format!("{where_}: {error}"))?;
    if palette.encode() != payload {
        return Err(format!("{where_}: re-encoded palette differs from the {}-byte payload", payload.len()));
    }
    for &word in palette.words() {
        if clut::rgba_to_word(clut::word_to_rgba(word)) != word & 0x7fff {
            return Err(format!("{where_}: the RGBA view of {word:#06x} does not return its RGB"));
        }
    }
    Ok(palette)
}

/// Checks the JSON and PNG views of one sampled bank against the documented shape.
fn check_sample(where_: &str, palette: &clut::Palette) {
    let document: serde_json::Value = serde_json::from_str(&palette.json()).expect("palette JSON parses");
    assert_eq!(document["colors"].as_array().map(Vec::len), Some(palette.words().len()), "{where_}: palette JSON color count");
    let png = palette.png_strip();
    assert!(png.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]), "{where_}: palette strip is not a PNG");
    let geometry = (&png[16..20], &png[20..24]);
    assert_eq!((be_u32(geometry.0), be_u32(geometry.1)), (16 * SWATCH_PIXELS, palette.rows_4bpp() as u32 * SWATCH_PIXELS), "{where_}: palette strip geometry");
}

fn be_u32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes(bytes.try_into().expect("four bytes"))
}

#[test]
fn retail_palettes_round_trip_byte_exactly() {
    let Some(root) = support::retail_root() else {
        return;
    };

    let mut archives = Vec::new();
    support::collect_archives(&root, &mut archives);
    archives.sort();
    assert!(!archives.is_empty(), "no .EMI containers under {}", root.display());

    let mut payloads = 0;
    let mut bytes = 0;
    let mut sample: Option<(String, clut::Palette)> = None;
    let mut failures = Vec::new();
    for archive_path in &archives {
        let archive = Archive::open(archive_path).expect("open retail archive");
        for (index, entry) in archive.entries().iter().enumerate() {
            if entry.file_type != 0 || !CLUT_BAND.contains(&entry.ram_ptr) {
                continue;
            }
            payloads += 1;
            bytes += entry.size as usize;
            let where_ = format!("{}#{index}", archive_path.display());
            match round_trip(&where_, &read_payload(archive_path, entry)) {
                Ok(palette) => {
                    if sample.is_none() && entry.size == SAMPLE_PAYLOAD {
                        sample = Some((where_, palette));
                    }
                }
                Err(failure) => failures.push(failure),
            }
        }
    }

    assert!(payloads >= CENSUS_PALETTES, "found {payloads} CLUT-band type-0 payloads, fewer than the census {CENSUS_PALETTES}");
    assert!(failures.is_empty(), "{}/{} CLUT-band type-0 payloads round-trip byte-exactly; {} failures:\n{}", payloads - failures.len(), payloads, failures.len(), failures.join("\n"));

    let (where_, palette) = sample.expect("a sampled band palette");
    check_sample(&where_, &palette);
    let out = support::out_dir("retail_palettes_round_trip_byte_exactly");
    support::write(&out, "sample-palette.png", palette.png_strip());
    support::write(&out, "sample-palette-colors.json", palette.json());
    support::write(&out, "sample-source.txt", &where_);

    // Shapes the band does not carry: a bank is a whole number of 16-color rows.
    assert!(matches!(clut::Palette::decode(&[0; 30]), Err(Error::PaletteNotRowAligned(15))));
    assert!(matches!(clut::Palette::decode(&[0; 31]), Err(Error::PaletteOddByteCount(31))));

    eprintln!(
        "clut gate: {payloads}/{payloads} CLUT-band type-0 payloads ({bytes} B) round-trip \
         byte-exactly (census palettes in the band: {CENSUS_PALETTES})"
    );
}
