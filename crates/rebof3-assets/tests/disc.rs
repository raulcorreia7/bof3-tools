//! The patched disc image on the owner's disc: the sealing method, and a disc patched with its own files. The
//! synthetic cases are in `psx-cd/tests/patch.rs`.
//!
//! The tests skip when no disc image is present. Point `BOF3_DISC_IMAGE` at its cue sheet, or place the image in the (`BOF3_REQUIRE_RETAIL=1` makes a skip a failure).

mod support;

use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use psx_cd::Image;
use psx_cd::cue::{self, TrackMode};
use psx_formats::sector::{self, RAW_SIZE};
use rebof3_assets::disc::{self, Replacement};

fn data_track_file(cue_path: &Path) -> PathBuf {
    let sheet = cue::read(cue_path).expect("the cue sheet reads");
    sheet.tracks.into_iter().find(|track| track.mode != TrackMode::Audio).expect("a data track").file
}

#[test]
fn every_form1_sector_of_the_retail_data_track_is_sealed() {
    let Some(cue_path) = support::retail_image() else {
        return;
    };
    let track = data_track_file(&cue_path);
    let sectors = fs::metadata(&track).expect("the data track exists").len() / RAW_SIZE as u64;
    let mut reader = BufReader::new(File::open(&track).expect("the data track opens"));
    let mut raw = [0_u8; RAW_SIZE];
    let (mut form1, mut form2) = (0_u64, 0_u64);
    let mut unsealed = Vec::new();
    for lba in 0..sectors {
        reader.read_exact(&mut raw).expect("a whole sector");
        if sector::is_form2(&raw) {
            form2 += 1;
        } else if sector::is_sealed(&raw) {
            form1 += 1;
        } else {
            unsealed.push(lba);
        }
    }
    // A sector past the last file or directory may be unsealed; the disc keeps such a sector as stored.
    let mut image = Image::open(&cue_path).expect("the disc opens");
    let covered_end = image
        .entries()
        .expect("the file table reads")
        .iter()
        .map(|entry| u64::from(entry.lba) + entry.sectors())
        .max()
        .expect("the disc has entries");
    eprintln!(
        "sealed: {form1} Form 1 sectors verify; {form2} Form 2 sectors left as stored; {} unsealed sectors past the last file",
        unsealed.len()
    );
    assert!(unsealed.iter().all(|&lba| lba >= covered_end), "unsealed sectors inside the file system: {unsealed:?}");
    assert!(form1 > 0);
}

#[test]
fn the_retail_disc_patched_with_its_own_files_is_byte_identical() {
    let Some(cue_path) = support::retail_image() else {
        return;
    };
    let mut image = Image::open(&cue_path).expect("the disc opens");
    let entries = image.entries().expect("the file table reads");
    let mut replacements = Vec::new();
    for entry in entries.iter().filter(|entry| !entry.is_directory && !entry.is_xa) {
        let bytes = image.read_file(entry).expect("the file reads");
        replacements.push(Replacement { path: entry.path.clone(), bytes });
    }
    let dir = support::out_dir("disc-the_retail_disc_patched_with_its_own_files_is_byte_identical");
    let output = dir.join("patched");
    let summary = disc::patch(&cue_path, &output, &replacements).expect("the disc patches");
    assert_eq!(summary.files, replacements.len());

    let source_dir = cue_path.parent().expect("the cue sheet has a directory");
    let mut names: Vec<PathBuf> =
        cue::read(&cue_path).expect("the cue sheet reads").tracks.into_iter().map(|t| t.file).collect();
    names.dedup();
    names.push(cue_path.clone());
    for name in names {
        let file_name = name.file_name().expect("a file name");
        assert_same_file(&source_dir.join(file_name), &output.join(file_name));
    }
    eprintln!("patched {} files, sealing {} sectors; the disc is byte-identical", summary.files, summary.sealed);
    fs::write(
        dir.join("patched.txt"),
        format!(
            "{} files replaced; {} sectors sealed; every track and the cue sheet are byte-identical.\n\
             The patched disc is a full copy and is not kept as an artifact.\n",
            summary.files, summary.sealed
        ),
    )
    .expect("the patch summary");
    fs::remove_dir_all(&output).expect("the patched copy is removed");
}

fn assert_same_file(original: &Path, patched: &Path) {
    let mut left = BufReader::new(File::open(original).expect("the original file opens"));
    let mut right = BufReader::new(File::open(patched).expect("the patched file opens"));
    assert_eq!(
        fs::metadata(original).expect("metadata").len(),
        fs::metadata(patched).expect("metadata").len(),
        "{} has another length",
        original.display()
    );
    let (mut left_block, mut right_block) = (vec![0_u8; 1 << 20], vec![0_u8; 1 << 20]);
    let mut offset = 0_u64;
    loop {
        let read = left.read(&mut left_block).expect("a read");
        if read == 0 {
            break;
        }
        right.read_exact(&mut right_block[..read]).expect("a matching read");
        assert!(left_block[..read] == right_block[..read], "{} differs near byte {offset}", original.display());
        offset += read as u64;
    }
}
