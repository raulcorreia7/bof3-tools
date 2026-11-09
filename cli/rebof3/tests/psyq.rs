// SPDX-License-Identifier: MIT
//! The Psy-Q file verbs (TIM, VAB, SEQ, SEP) over small synthetic inputs: each export reads back to
//! the bytes it came from.

use std::path::{Path, PathBuf};
use std::process::Command;

use psx_formats::sep::{Event, Package, Sequence};

fn bof3(dir: &Path, args: &[&str]) -> String {
    let run = Command::new(env!("CARGO_BIN_EXE_bof3")).args(args).current_dir(dir).output().expect("bof3 runs");
    let text = String::from_utf8_lossy(&run.stdout).into_owned() + &String::from_utf8_lossy(&run.stderr);
    assert!(run.status.success(), "bof3 {args:?} failed: {text}");
    text
}

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bof3-psyq-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Load argument of a two-rectangle sheet at column 22, row 8, one rectangle to a row.
const LOAD: &str = "0x16080100";

fn sheet() -> Vec<u8> {
    (0..2 * 1024u16).flat_map(|word| word.wrapping_mul(7).to_le_bytes()).collect()
}

fn block_words() -> Vec<u8> {
    (0..32u16).flat_map(|word| (0x8000 | word).to_le_bytes()).collect()
}

#[test]
fn a_tim_goes_back_to_the_texture_and_the_palette() {
    let dir = folder("tim");
    std::fs::write(dir.join("t.img"), sheet()).unwrap();
    std::fs::write(dir.join("block.clut"), block_words()).unwrap();

    bof3(&dir, &["assets", "export", "tim", "t.img", "--load-arg", LOAD, "--depth", "4", "--palette", "block.clut", "--palette-offset", "16", "--clut", "160,483"]);
    let said = bof3(&dir, &["assets", "import", "tim", "t.tim", "-o", "back.img", "--load-arg", LOAD, "--palette", "row.bin"]);
    assert!(said.contains("load argument 0x16080100"), "{said}");
    assert!(said.contains("CLUT at (160, 483)"), "{said}");
    assert_eq!(std::fs::read(dir.join("back.img")).unwrap(), sheet());
    assert_eq!(std::fs::read(dir.join("row.bin")).unwrap(), block_words()[32..64]);

    bof3(&dir, &["assets", "import", "tim", "t.tim", "-o", "again.img", "--palette", "spliced.clut", "--clut-block", "block.clut", "--palette-offset", "16"]);
    assert_eq!(std::fs::read(dir.join("spliced.clut")).unwrap(), block_words(), "an unchanged row leaves the CLUT block as it was");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_texture_without_a_join_has_no_clut_and_no_guess() {
    let dir = folder("plain");
    std::fs::write(dir.join("t.img"), sheet()).unwrap();
    bof3(&dir, &["assets", "export", "tim", "t.img", "--load-arg", LOAD]);
    let tim = psx_formats::gfx::tim::File::read(&std::fs::read(dir.join("t.tim")).unwrap()).unwrap();
    assert!(tim.clut.is_none());
    assert_eq!((tim.image.x, tim.image.y), (704, 256));
    let run = Command::new(env!("CARGO_BIN_EXE_bof3")).args(["assets", "export", "tim", "t.img", "--load-arg", LOAD, "--depth", "4", "--palette", "t.img"]).current_dir(&dir).output().unwrap();
    assert!(!run.status.success(), "a CLUT place is never guessed");
    assert!(String::from_utf8_lossy(&run.stderr).contains("--clut"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_vab_file_splits_into_the_pair_and_joins_back() {
    let dir = folder("vab");
    let (header, body) = psx_formats::vab::encode_bank(3, &[], &[vec![0u8; 16], vec![1u8; 32]]).unwrap();
    std::fs::write(dir.join("b.vh"), &header).unwrap();
    std::fs::write(dir.join("b.vb"), &body).unwrap();

    bof3(&dir, &["assets", "export", "vab", "b.vh", "b.vb"]);
    let whole = std::fs::read(dir.join("b.vab")).unwrap();
    assert_eq!(whole, [header.clone(), body.clone()].concat());

    bof3(&dir, &["assets", "import", "vab", "b.vab", "-o", "c.vh", "--body", "c.vb"]);
    assert_eq!(std::fs::read(dir.join("c.vh")).unwrap(), header);
    assert_eq!(std::fs::read(dir.join("c.vb")).unwrap(), body);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sequences_go_through_seq_and_sep_files() {
    let dir = folder("seq");
    let end = Event { status: 0xff, meta_type: 0x2f, status_byte: true, ..Event::default() };
    let note = Event { status: 0x90, data1: 60, data2: 100, status_byte: true, ..Event::default() };
    let sequence = |seq_id| Sequence { seq_id, resolution: 48, tempo_us: 500_000, time_num: 4, time_den: 2, events: vec![note.clone(), end.clone()] };
    let package = Package { sequences: vec![sequence(1), sequence(5)] }.encode();
    std::fs::write(dir.join("p.seq"), &package).unwrap();

    bof3(&dir, &["assets", "export", "seq", "p.seq", "--index", "0", "-o", "a.seq"]);
    bof3(&dir, &["assets", "export", "seq", "p.seq", "--index", "1", "-o", "b.seq"]);
    bof3(&dir, &["assets", "import", "seq", "a.seq:1", "b.seq:5", "-o", "back.bin"]);
    assert_eq!(std::fs::read(dir.join("back.bin")).unwrap(), package);

    bof3(&dir, &["assets", "export", "sep", "p.seq"]);
    bof3(&dir, &["assets", "import", "sep", "p.sep", "-o", "sep.bin"]);
    assert_eq!(std::fs::read(dir.join("sep.bin")).unwrap(), package);
    let _ = std::fs::remove_dir_all(&dir);
}
