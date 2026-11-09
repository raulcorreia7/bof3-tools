//! The Psy-Q files over the retail disc: every joined texture through TIM and back, every sound bank through VAB and VH/VB, every sequence package through SEP and SEQ. The tests skip when no retail archive directory is present (`BOF3_DISC_DIR`).

mod support;

use std::collections::BTreeMap;
use std::fs;

use rebof3_emi::{Archive, texture};
use rebof3_formats::joins::joined_rows;
use rebof3_formats::vram::{self, Placement};
use rebof3_formats::{Format, classify, clut, sep, vab};

/// One archive of the disc: its entries and payloads.
struct Disc {
    path: String,
    archive: Archive,
    payloads: Vec<Vec<u8>>,
}

impl Disc {
    fn slices(&self) -> Vec<&[u8]> {
        self.payloads.iter().map(Vec::as_slice).collect()
    }
}

fn read_disc() -> Option<Vec<Disc>> {
    let root = support::retail_root()?;
    let mut paths = rebof3_formats::coverage::archives_under(&root);
    paths.sort();
    Some(
        paths
            .iter()
            .map(|path| {
                let bytes = fs::read(path).expect("read the retail archive");
                let archive = Archive::parse(&bytes).expect("an archive");
                let payloads = (0..archive.entries().len())
                    .map(|slot| archive.payload(&bytes, slot).expect("a payload").to_vec())
                    .collect();
                let relative = path.strip_prefix(&root).unwrap_or(path).to_string_lossy().replace('\\', "/");
                Disc { path: relative, archive, payloads }
            })
            .collect(),
    )
}

/// Every joined row: 883 rows of 203 textures.
#[test]
fn retail_joined_textures_round_trip_through_tim() {
    let Some(discs) = read_disc() else { return };
    let blocks: BTreeMap<(&str, usize), &[u8]> = discs
        .iter()
        .flat_map(|disc| {
            disc.payloads.iter().enumerate().map(|(slot, payload)| ((disc.path.as_str(), slot), payload.as_slice()))
        })
        .collect();
    let (mut textures, mut rows, mut spliced) = (0, 0, 0);
    for disc in &discs {
        let slices = disc.slices();
        let joined = joined_rows(&disc.path, disc.archive.entries(), &slices);
        for (&slot, uses) in &joined {
            textures += 1;
            let entry = &disc.archive.entries()[slot];
            for row in uses {
                rows += 1;
                let tim = vram::texture_tim(entry.ram_ptr, &disc.payloads[slot], row).expect("a TIM");
                let back = vram::import_tim(&tim).expect("the TIM reads back");
                let name = format!("{} #{slot} at {:?}", disc.path, row.clut);
                assert_eq!(back.payload, disc.payloads[slot], "{name}: the texture");
                assert_eq!(back.placement, Placement::of_load_argument(entry.ram_ptr), "{name}: the placement");
                assert_eq!(back.placement.load_argument(entry.ram_ptr), entry.ram_ptr, "{name}: the load argument");
                assert_eq!(back.depth, Some(row.depth), "{name}: the depth");
                let block = back.clut.expect("a CLUT block");
                assert_eq!((block.x, block.y), (row.clut.x, row.clut.y), "{name}: the CLUT place");
                assert_eq!(block.words, row.words, "{name}: the CLUT row");

                let stored = blocks.get(&(row.palette_archive.as_str(), row.palette)).expect("the palette entry");
                let block_entry = discs
                    .iter()
                    .find(|other| other.path == row.palette_archive)
                    .map(|other| &other.archive.entries()[row.palette])
                    .expect("the palette archive");
                let at = texture::ClutBlock::new(row.palette, block_entry.ram_ptr, stored.len())
                    .and_then(|clut_block| texture::arena_word(row.clut).map(|word| word - clut_block.arena_word))
                    .expect("the row lies in its CLUT block");
                assert_eq!(clut::splice(stored, at, &block.words).expect("splices"), *stored, "{name}: the block");
                spliced += 1;
            }
        }
    }
    assert_eq!((textures, rows, spliced), (203, 883, 883), "joined textures, rows and blocks spliced");
}

/// Every texture upload, joined or not, as a 15-bit TIM without a CLUT: 593 uploads.
#[test]
fn retail_uploads_round_trip_through_a_plain_tim() {
    let Some(discs) = read_disc() else { return };
    let mut uploads = 0;
    for disc in &discs {
        for (slot, entry) in disc.archive.entries().iter().enumerate() {
            if classify(entry, &disc.payloads[slot]) != Format::VramUpload {
                continue;
            }
            uploads += 1;
            let tim = vram::texture_tim_direct(entry.ram_ptr, &disc.payloads[slot]).expect("a TIM");
            let back = vram::import_tim(&tim).expect("the TIM reads back");
            let name = format!("{} #{slot}", disc.path);
            assert_eq!(back.payload, disc.payloads[slot], "{name}");
            assert_eq!(back.placement.load_argument(entry.ram_ptr), entry.ram_ptr, "{name}");
            assert!(back.clut.is_none() && back.depth.is_none(), "{name}: a plain TIM has no CLUT");
        }
    }
    assert_eq!(uploads, 593, "VRAM uploads on the disc");
}

/// Every bank whose header and body share a load argument, as one VAB file and as the VH/VB pair.
#[test]
fn retail_banks_round_trip_through_vab_and_vh_vb() {
    let Some(discs) = read_disc() else { return };
    let mut banks = 0;
    for disc in &discs {
        let entries = disc.archive.entries();
        for (slot, entry) in entries.iter().enumerate() {
            if classify(entry, &disc.payloads[slot]) != Format::VabHeader {
                continue;
            }
            let body = entries.iter().enumerate().find_map(|(other, candidate)| {
                (candidate.ram_ptr == entry.ram_ptr && classify(candidate, &disc.payloads[other]) == Format::VabBody)
                    .then_some(&disc.payloads[other])
            });
            let Some(body) = body else { continue };
            banks += 1;
            let name = format!("{} #{slot}", disc.path);
            vab::check_pair(&disc.payloads[slot], body).unwrap_or_else(|error| panic!("{name}: {error}"));
            let whole = vab::standard_file(&disc.payloads[slot], body);
            assert_eq!(whole.len(), disc.payloads[slot].len() + body.len(), "{name}");
            let (header, split) = vab::split_file(&whole).unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!((&header, &split), (&disc.payloads[slot], body), "{name}: the pair");
        }
    }
    assert_eq!(banks, 1020, "banks with a header and a body");
}

/// Every sequence package as a SEP file, and each of its sequences as a SEQ file that rebuilds it.
#[test]
fn retail_sequence_packages_round_trip_through_sep_and_seq() {
    let Some(discs) = read_disc() else { return };
    let (mut packages, mut sequences) = (0, 0);
    for disc in &discs {
        for (slot, entry) in disc.archive.entries().iter().enumerate() {
            if classify(entry, &disc.payloads[slot]) != Format::SepPackage {
                continue;
            }
            packages += 1;
            let payload = &disc.payloads[slot];
            let name = format!("{} #{slot}", disc.path);
            assert_eq!(&sep::psyq::export_sep(payload).expect("a SEP file"), payload, "{name}: export");
            assert_eq!(&sep::psyq::import_sep(payload).expect("a SEP file"), payload, "{name}: import");

            let package = psx_formats::sep::Package::parse(payload).expect("a package");
            let files: Vec<Vec<u8>> = (0..package.sequences.len())
                .map(|index| sep::psyq::export_seq(payload, index).expect("a SEQ file"))
                .collect();
            let parts: Vec<(&[u8], u16)> = files
                .iter()
                .zip(&package.sequences)
                .map(|(file, sequence)| (file.as_slice(), sequence.seq_id))
                .collect();
            assert_eq!(&sep::psyq::import_seq(&parts).expect("the SEQ files"), payload, "{name}: SEQ files");
            sequences += files.len();
        }
    }
    assert_eq!((packages, sequences), (119, 476), "sequence packages and their sequences");
}
