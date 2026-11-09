//! A format module depends on the registry and the primitives, never on another format module
//! (`docs/adr/0002-self-contained-formats.md`, rule 1).

use std::fs;
use std::path::Path;

/// The modules of `src/` that hold one format each.
const FORMATS: [&str; 16] = [
    "cells",
    "clut",
    "cue",
    "encounter",
    "game_tables",
    "map",
    "message",
    "residue",
    "resources",
    "scene",
    "script",
    "sep",
    "sprite_bank",
    "vab",
    "vram",
    "window",
];

/// The modules a format may import: the registry row types, the identity check, the error type and
/// the glTF writer.
const SHARED: [&str; 4] = ["registry", "identity", "error", "gltf"];

/// The source files of one format: `<format>.rs`, or every file of `<format>/`.
fn sources(source: &Path, format: &str) -> Vec<String> {
    let file = source.join(format!("{format}.rs"));
    if file.is_file() {
        return vec![fs::read_to_string(file).expect("the format module")];
    }
    fs::read_dir(source.join(format))
        .expect("the format folder")
        .map(|entry| fs::read_to_string(entry.expect("a source file").path()).expect("a readable source"))
        .collect()
}

#[test]
fn a_format_module_imports_no_other_format() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for format in FORMATS {
        for text in sources(&source, format) {
            for other in FORMATS.iter().filter(|other| **other != format) {
                for needle in [format!("crate::{other}"), format!("crate::{{{other}"), format!("super::{other}")] {
                    assert!(!text.contains(&needle), "{format} imports {other} ({needle})");
                }
            }
            for line in text.lines().filter(|line| line.trim_start().starts_with("use crate::")) {
                let own = line.contains(&format!("crate::{format}"));
                let allowed = own || SHARED.iter().any(|shared| line.contains(&format!("crate::{shared}")));
                assert!(allowed, "{format}: `{line}` imports a module that is not shared");
            }
        }
    }
}
