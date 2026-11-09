//! A format module depends on the registry and the primitives, never on another format module
//! (`docs/adr/0002-self-contained-formats.md`, rule 1).

use std::fs;
use std::path::Path;

/// The modules of `src/` that hold one format each.
const FORMATS: [&str; 13] = [
    "cells",
    "clut",
    "cue",
    "encounter",
    "map",
    "message",
    "resources",
    "scene",
    "script",
    "sep",
    "sprite_bank",
    "vab",
    "vram",
];

/// The modules a format may import: the registry row types, the identity check and the error type.
const SHARED: [&str; 3] = ["registry", "identity", "error"];

#[test]
fn a_format_module_imports_no_other_format() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for format in FORMATS {
        let text = fs::read_to_string(source.join(format!("{format}.rs"))).expect("the format module");
        for other in FORMATS.iter().filter(|other| **other != format) {
            for needle in [format!("crate::{other}"), format!("crate::{{{other}"), format!("super::{other}")] {
                assert!(!text.contains(&needle), "{format} imports {other} ({needle})");
            }
        }
        for line in text.lines().filter(|line| line.trim_start().starts_with("use crate::")) {
            let allowed = SHARED.iter().any(|shared| line.contains(&format!("crate::{shared}")));
            assert!(allowed, "{format}: `{line}` imports a module that is not shared");
        }
    }
}
