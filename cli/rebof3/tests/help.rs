// SPDX-License-Identifier: MIT
//! The help text is a contract: every command prints its own help and the output below must not
//! change unless `tests/help.snap` changes with it. `UPDATE_SNAPSHOT=1 cargo test -p rebof3 --test help`
//! rewrites the snapshot.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::Command;

const DISC: &[&str] = &["extract", "lba-json", "checksum", "verify", "rebuild"];
const EMI: &[&str] = &["extract", "pack", "list", "vram", "clut", "vab", "sep", "coverage"];
const ASSETS: &[&str] = &[
    "extract",
    "extract-all",
    "coverage",
    "kinds",
    "terrain",
    "soundfont",
    "tables",
    "encounters",
    "export",
    "import",
];
const EXPORT: &[&str] = &[
    "texture",
    "palette",
    "terrain",
    "vag",
    "sequence",
    "message",
    "strings",
    "characters",
    "character-bank",
    "sprite-sheet",
];
const IMPORT: &[&str] = &[
    "texture",
    "palette",
    "terrain",
    "vag",
    "table",
    "encounters",
    "layout",
    "emi",
    "sequence",
    "disc",
    "soundfont",
    "message",
    "strings",
    "characters",
    "character-bank",
    "sprite-sheet",
];

fn invocations() -> Vec<Vec<&'static str>> {
    let mut all: Vec<Vec<&str>> = vec![vec![], vec!["--help"], vec!["--version"], vec!["disc", "--help"]];
    all.extend(DISC.iter().map(|verb| vec!["disc", verb, "--help"]));
    all.push(vec!["emi", "--help"]);
    all.extend(EMI.iter().map(|verb| vec!["emi", verb, "--help"]));
    all.push(vec!["assets", "--help"]);
    all.extend(ASSETS.iter().map(|verb| vec!["assets", verb, "--help"]));
    all.extend(EXPORT.iter().map(|kind| vec!["assets", "export", kind, "--help"]));
    all.extend(IMPORT.iter().map(|kind| vec!["assets", "import", kind, "--help"]));
    all.push(vec!["assets", "kinds"]);
    all.push(vec!["observe", "--help"]);
    all
}

fn render() -> String {
    let mut out = String::new();
    for args in invocations() {
        let run = Command::new(env!("CARGO_BIN_EXE_bof3")).args(&args).output().expect("bof3 runs");
        writeln!(out, "$ bof3 {}", args.join(" ")).unwrap();
        writeln!(out, "[exit {}]", run.status.code().unwrap_or(-1)).unwrap();
        out.push_str(&String::from_utf8_lossy(&run.stdout));
        out.push_str(&String::from_utf8_lossy(&run.stderr));
        out.push('\n');
    }
    out
}

#[test]
fn every_help_text_matches_the_snapshot() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/help.snap");
    let actual = render();
    if std::env::var_os("UPDATE_SNAPSHOT").is_some() {
        std::fs::write(&path, &actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .expect("tests/help.snap exists; run with UPDATE_SNAPSHOT=1")
        .replace("\r\n", "\n");
    assert!(
        actual == expected,
        "help output changed; diff tests/help.snap against the new output, or rerun with UPDATE_SNAPSHOT=1"
    );
}
