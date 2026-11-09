//! Shared fixtures and retail-archive discovery for the integration tests.
//!
//! Each integration test compiles its own copy of this module and uses a subset
//! of it, so unused items are expected.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use rebof3_emi::Archive;

/// The path of the retail archives, relative to the checkout root.
pub const RETAIL_SUFFIX: &str = "raw/disc/BIN";

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
