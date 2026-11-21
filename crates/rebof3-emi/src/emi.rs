//! The EMI container table from the `emi` crate, and the discovery of containers on disk.

pub use ::emi::{Archive, Entry, HEADER_SIZE, MAGIC, SECTOR_SIZE, TOC_ENTRY_SIZE, TYPE_CUE, TYPE_LZ77, TYPE_SEP, TYPE_VAB_BODY, TYPE_VAB_HEADER, TYPE_VRAM, align_sector};
#[cfg(feature = "std")]
pub use ::emi::{ensure_parent, entry_name, read_entry, type_extension, upload};

/// Every `.EMI` container under `root`, through subdirectories, sorted.
#[cfg(feature = "std")]
pub fn archives_under(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    collect_archives(root, &mut found);
    found.sort();
    found
}

#[cfg(feature = "std")]
fn collect_archives(root: &std::path::Path, found: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_archives(&path, found);
        } else if path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("EMI")) {
            found.push(path);
        }
    }
}
