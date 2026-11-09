//! Retail-archive discovery for the integration tests.
//!
//! Each integration test compiles its own copy of this module and uses a subset of it.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

/// The path of the retail archives, relative to the checkout root.
pub const RETAIL_SUFFIX: &str = "raw/disc/BIN";

/// The retail archive root: `BOF3_EMI_ROOT` when set, otherwise the
/// checkout's `raw/disc/BIN` found by walking up from the crate directory.
pub fn retail_root() -> Option<PathBuf> {
    if let Some(root) = std::env::var_os("BOF3_EMI_ROOT") {
        let root = PathBuf::from(root);
        return root.is_dir().then_some(root);
    }
    let mut directory = Some(Path::new(env!("CARGO_MANIFEST_DIR")));
    while let Some(current) = directory {
        let root = current.join(RETAIL_SUFFIX);
        if root.is_dir() {
            return Some(root);
        }
        directory = current.parent();
    }
    None
}

/// Appends every `.EMI` container under `root` to `found`, through subdirectories.
pub fn collect_archives(root: &Path, found: &mut Vec<PathBuf>) {
    found.extend(rebof3_emi::archives_under(root));
}

/// An empty folder for one test's files: `target/tmp/<test>`.
pub fn out_dir(test: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(test);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the output folder");
    dir
}

/// Writes one file of a test's output.
pub fn write(dir: &Path, file: &str, bytes: impl AsRef<[u8]>) {
    std::fs::write(dir.join(file), bytes).unwrap_or_else(|error| panic!("cannot write {file}: {error}"));
}
