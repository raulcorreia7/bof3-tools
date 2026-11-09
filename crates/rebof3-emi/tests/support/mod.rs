//! Shared fixtures and retail-archive discovery for the integration tests.
//!
//! Each integration test compiles its own copy of this module and uses a subset
//! of it, so unused items are expected.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use rebof3_emi::Archive;

pub fn fixture() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(b"MATH_TBL");
    for (data, file_type, ram_ptr) in [(&b"ABCDx"[..], 6_u16, 0x8010_0000_u32), (&b"yz"[..], 0_u16, 0)] {
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&ram_ptr.to_le_bytes());
        let mut first = [0; 4];
        first[..data.len().min(4)].copy_from_slice(&data[..data.len().min(4)]);
        bytes.extend_from_slice(&first);
        bytes.extend_from_slice(&file_type.to_le_bytes());
        bytes.extend_from_slice(&0x2e2e_u16.to_le_bytes());
    }
    bytes.resize(0x800, 0x2e);
    bytes.extend_from_slice(b"ABCDx");
    bytes.resize(0x1000, 0x5f);
    bytes.extend_from_slice(b"yz");
    bytes.resize(0x1800, 0x5f);
    bytes
}

/// Appends every `.EMI` container under `root` to `found`, through subdirectories.
pub fn collect_archives(root: &Path, found: &mut Vec<PathBuf>) {
    found.extend(rebof3_emi::archives_under(root));
}

/// First offset where two byte runs differ, or `None` when they are equal.
pub fn first_mismatch(left: &[u8], right: &[u8]) -> Option<usize> {
    let common = left.len().min(right.len());
    for index in 0..common {
        if left[index] != right[index] {
            return Some(index);
        }
    }
    (left.len() != right.len()).then_some(common)
}

/// One retail payload: archive path, entry index, payload bytes.
pub type Payload = (PathBuf, usize, Vec<u8>);

/// Every type-1 payload in the retail archives, sorted by archive path.
pub fn type_1_payloads(root: &Path) -> Vec<Payload> {
    let mut archives = Vec::new();
    collect_archives(root, &mut archives);
    archives.sort();
    let mut payloads = Vec::new();
    for path in archives {
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        let Ok(archive) = Archive::open(&path) else {
            continue;
        };
        for (index, entry) in archive.entries().iter().enumerate() {
            if entry.file_type != 1 {
                continue;
            }
            let start = entry.offset as usize;
            let end = start + entry.size as usize;
            if end <= bytes.len() {
                payloads.push((path.clone(), index, bytes[start..end].to_vec()));
            }
        }
    }
    payloads
}

/// An empty folder for one test's files: `target/tmp/<test>`.
pub fn out_dir(test: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(test);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("the output folder");
    dir
}

/// Writes one file of a test's output.
pub fn write(dir: &Path, file: &str, bytes: impl AsRef<[u8]>) {
    fs::write(dir.join(file), bytes).unwrap_or_else(|error| panic!("cannot write {file}: {error}"));
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
