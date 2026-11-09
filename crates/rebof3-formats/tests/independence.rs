//! A format module depends on the registry and the primitives, never on another format module

use std::fs;
use std::path::Path;

/// The modules of `src/` that hold one format each.
const FORMATS: [&str; 19] = [
    "cells",
    "sprite_palettes",
    "characters",
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
    "sprite",
    "character_sprites",
    "vab",
    "vram",
    "window",
];

/// The modules a format may import.
const SHARED: [&str; 6] = ["registry", "identity", "error", "gltf", "text", "animation"];

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

/// Whether `text` names the path `needle`, not a longer name that starts with it (`sprite` and
fn mentions(text: &str, needle: &str) -> bool {
    text.match_indices(needle).any(|(start, _)| {
        let next = text[start + needle.len()..].chars().next();
        !next.is_some_and(|character| character.is_alphanumeric() || character == '_')
    })
}

#[test]
fn a_format_module_imports_no_other_format() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for format in FORMATS {
        for text in sources(&source, format) {
            for other in FORMATS.iter().filter(|other| **other != format) {
                for needle in [format!("crate::{other}"), format!("crate::{{{other}"), format!("super::{other}")] {
                    assert!(!mentions(&text, &needle), "{format} imports {other} ({needle})");
                }
            }
            for line in text.lines().filter(|line| line.trim_start().starts_with("use crate::")) {
                let own = mentions(line, &format!("crate::{format}"));
                let allowed = own || SHARED.iter().any(|shared| mentions(line, &format!("crate::{shared}")));
                assert!(allowed, "{format}: `{line}` imports a module that is not shared");
            }
        }
    }
}
