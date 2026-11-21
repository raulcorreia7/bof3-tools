//! Finding a disc image and extracting its files.

use std::fmt;
use std::path::{Path, PathBuf};

use psx_cd::{Entry, Image, metadata};

/// Where the disc image lives, relative to the repository root.
pub const IMAGE_DIR: &str = "inputs/external";
/// Folders searched for the disc image, in order, when none is named.
pub const IMAGE_DIRS: [&str; 2] = [IMAGE_DIR, "disk"];

/// Why a disc image was not found or did not extract.
#[derive(Debug)]
pub enum DiscError {
    /// No searched folder holds an image: each folder and why it was refused.
    NoImage(Vec<(&'static str, psx_cd::Error)>),
    /// The image at `path` does not read or extract.
    Image { path: PathBuf, error: psx_cd::Error },
    /// The project XML does not write.
    Project(psx_cd::Error),
}

impl fmt::Display for DiscError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoImage(reasons) => {
                let reasons: Vec<String> = reasons.iter().map(|(directory, error)| format!("{directory}: {error}")).collect();
                write!(f, "no disc image found ({})", reasons.join("; "))
            }
            Self::Image { path, error } => write!(f, "{}: {error}", path.display()),
            Self::Project(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for DiscError {}

/// The disc image in the first of [`IMAGE_DIRS`] below `base` that holds one.
pub fn find_image(base: &Path) -> Result<PathBuf, DiscError> {
    let mut reasons = Vec::new();
    for directory in IMAGE_DIRS {
        match psx_cd::discover_image(&base.join(directory)) {
            Ok(path) => return Ok(path),
            Err(error) => reasons.push((directory, error)),
        }
    }
    Err(DiscError::NoImage(reasons))
}

/// Extracts every file of `input` into `output` and writes the project XML to `project`; returns the entries.
pub fn extract(input: &Path, output: &Path, project: &Path) -> Result<Vec<Entry>, DiscError> {
    let at = |error| DiscError::Image { path: input.to_path_buf(), error };
    let mut image = Image::open(input).map_err(at)?;
    let entries = image.entries().map_err(at)?;
    image.extract(output).map_err(at)?;
    metadata::write_project(project, &entries, input.file_name().and_then(|value| value.to_str()).unwrap_or("disc.bin")).map_err(DiscError::Project)?;
    Ok(entries)
}
