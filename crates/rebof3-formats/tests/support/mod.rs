//! Retail-archive discovery for the integration tests.
//!
//! Each integration test compiles its own copy of this module and uses a subset of it.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

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

// ---- Retail inputs ----------------------------------------------------------------------------
//
// Two variables name the retail data: `BOF3_DISC_DIR` (the extracted disc, which holds `BIN/`) and
// `BOF3_DISC_IMAGE` (the disc's cue sheet). Unset, a retail test skips with a message. Set but
// invalid, it panics with the path. `BOF3_REQUIRE_RETAIL=1` turns every skip into a failure.

/// The variable that names the extracted disc.
pub const DISC_DIR_VARIABLE: &str = "BOF3_DISC_DIR";

/// The variable that names the disc's cue sheet.
pub const DISC_IMAGE_VARIABLE: &str = "BOF3_DISC_IMAGE";

/// The variable that turns a skipped retail test into a failure.
pub const REQUIRE_RETAIL_VARIABLE: &str = "BOF3_REQUIRE_RETAIL";

/// The folder of the extracted disc that holds the EMI archives.
const ARCHIVE_FOLDER: &str = "BIN";

/// Reports a skipped retail test, or fails it when `BOF3_REQUIRE_RETAIL` is set.
fn skip(variable: &str) -> Option<PathBuf> {
    let required = std::env::var_os(REQUIRE_RETAIL_VARIABLE).is_some_and(|value| value == "1");
    assert!(!required, "{REQUIRE_RETAIL_VARIABLE}=1 but {variable} is not set");
    eprintln!("skipping: set {variable} to run the retail test");
    None
}

/// The extracted disc named by `BOF3_DISC_DIR`; `None` (a skip) when unset, a panic when it has no `BIN/`.
pub fn retail_disc() -> Option<PathBuf> {
    let Some(value) = std::env::var_os(DISC_DIR_VARIABLE) else { return skip(DISC_DIR_VARIABLE) };
    let disc = PathBuf::from(value);
    assert!(disc.join(ARCHIVE_FOLDER).is_dir(), "{DISC_DIR_VARIABLE}={} is not an extracted disc: it has no {ARCHIVE_FOLDER}/ folder", disc.display());
    Some(disc)
}

/// The EMI archive root, `$BOF3_DISC_DIR/BIN`, under the same rules as [`retail_disc`].
pub fn retail_root() -> Option<PathBuf> {
    retail_disc().map(|disc| disc.join(ARCHIVE_FOLDER))
}

/// The disc's cue sheet named by `BOF3_DISC_IMAGE`; `None` (a skip) when unset, a panic when the file is missing.
pub fn retail_image() -> Option<PathBuf> {
    let Some(value) = std::env::var_os(DISC_IMAGE_VARIABLE) else { return skip(DISC_IMAGE_VARIABLE) };
    let cue = PathBuf::from(value);
    assert!(cue.is_file(), "{DISC_IMAGE_VARIABLE}={} is not a file", cue.display());
    Some(cue)
}
