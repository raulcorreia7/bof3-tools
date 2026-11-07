//! Shared output folders for the integration tests.

#![allow(dead_code)]

use std::fs;
use std::ops::Deref;
use std::path::{Path, PathBuf};

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

/// A folder for a tree of retail files. It is emptied when the test starts and removed when
/// the test ends, so a run keeps no copy of the disc.
pub struct Scratch(PathBuf);

/// An empty `target/tmp/<test>` that the returned value removes on drop.
pub fn scratch_dir(test: &str) -> Scratch {
    Scratch(out_dir(test))
}

impl Deref for Scratch {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The retail disc image, from `BOF3_DISC_IMAGE` or the checkout's `inputs/external`.
pub fn retail_image() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("BOF3_DISC_IMAGE") {
        return Some(PathBuf::from(path));
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .find_map(|directory| psx_cd::discover_image(&directory.join("inputs/external")).ok())
}
