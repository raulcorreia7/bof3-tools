//! Patched disc images. The writer is `psx_cd::patch`; this module keeps the names the asset tools use,
//! with this crate's error type.

use std::path::Path;

pub use psx_cd::patch::{Replacement, Summary};

use crate::error::Result;

/// Writes a patched copy of the disc named by `cue_path` into the directory `output`. See `psx_cd::patch::apply`.
pub fn patch(cue_path: &Path, output: &Path, replacements: &[Replacement]) -> Result<Summary> {
    Ok(psx_cd::patch::apply(cue_path, output, replacements)?)
}
