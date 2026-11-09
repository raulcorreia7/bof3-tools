//! Retail-archive discovery for the integration tests.

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
