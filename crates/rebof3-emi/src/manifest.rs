//! The `emi.json` manifest an extraction writes next to the payloads and a pack reads back (`wiki: formats/emi`).

use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::emi::{Entry, entry_name};
use crate::error::{Error, Result};
use crate::pack::PackEntry;

/// Manifest schema version written under `version`.
const SCHEMA_VERSION: u32 = 1;

#[derive(Serialize)]
struct Manifest {
    archive_version: u32,
    entries: Vec<ManifestEntry>,
    version: u32,
}

#[derive(Serialize)]
struct ManifestEntry {
    first4: u32,
    index: usize,
    name: String,
    ram_ptr: u32,
    size: u32,
    #[serde(rename = "type")]
    file_type: u16,
}

/// The manifest text for a container with `entries`.
pub(crate) fn to_json(archive_version: u32, entries: &[Entry], typed: bool) -> String {
    let manifest = Manifest {
        archive_version,
        entries: entries
            .iter()
            .enumerate()
            .map(|(index, entry)| ManifestEntry {
                first4: entry.first4,
                index,
                name: entry_name(index, entry.file_type, typed),
                ram_ptr: entry.ram_ptr,
                size: entry.size,
                file_type: entry.file_type,
            })
            .collect(),
        version: SCHEMA_VERSION,
    };
    serde_json::to_string_pretty(&manifest).expect("manifest model is serializable")
}

/// Loads a manifest's named entries and validates every one of them against `folder` before any payload is read or any output is created.
pub fn manifest_entries(folder: impl AsRef<Path>, manifest_path: impl AsRef<Path>) -> Result<(Vec<PackEntry>, u32)> {
    let value: Value = serde_json::from_slice(&fs::read(manifest_path.as_ref())?)
        .map_err(|_| Error::InvalidArchive("invalid manifest JSON"))?;
    let entries = value
        .get("entries")
        .and_then(Value::as_array)
        .ok_or(Error::InvalidArchive("manifest entries must be an array"))?;
    let archive_version = json_number(value.get("archive_version"));
    let root = canonical_folder(folder.as_ref())?;
    let mut inputs = Vec::new();
    for entry in entries {
        let Some(name) = entry.get("name").and_then(Value::as_str) else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        inputs.push(PackEntry {
            path: manifest_input_path(&root, name)?,
            file_type: json_number(entry.get("type")),
            ram_ptr: json_number(entry.get("ram_ptr")),
        });
    }
    if inputs.is_empty() {
        return Err(Error::InvalidArchive("manifest has no named entries"));
    }
    Ok((inputs, archive_version))
}

/// The canonical input folder: the containment root every manifest name is compared against as path components.
fn canonical_folder(folder: &Path) -> Result<PathBuf> {
    let root = fs::canonicalize(folder)
        .map_err(|error| Error::ManifestInput(format!("input folder {}: {error}", folder.display())))?;
    if !root.is_dir() {
        return Err(Error::ManifestInput(format!("input folder {} is not a directory", folder.display())));
    }
    Ok(root)
}

/// Resolves one manifest name inside `root`, rejecting every name that could read or delete a file the caller did not put in the input folder.
fn manifest_input_path(root: &Path, name: &str) -> Result<PathBuf> {
    for component in Path::new(name).components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir => {
                return Err(Error::ManifestInput(format!("manifest entry {name:?} has a parent-directory component")));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(Error::ManifestInput(format!("manifest entry {name:?} is not a relative path")));
            }
        }
    }
    let candidate = root.join(name);
    let resolved = fs::canonicalize(&candidate)
        .map_err(|error| Error::ManifestInput(format!("{}: {error}", candidate.display())))?;
    if !resolved.starts_with(root) {
        return Err(Error::ManifestInput(format!(
            "manifest entry {name:?} resolves outside the input folder: {}",
            resolved.display()
        )));
    }
    if !fs::metadata(&resolved)?.is_file() {
        return Err(Error::ManifestInput(format!(
            "manifest entry {name:?} is not a regular file: {}",
            resolved.display()
        )));
    }
    Ok(resolved)
}

/// A JSON number as the integer type the manifest field holds, or `0`.
fn json_number<T: TryFrom<u64> + Default>(value: Option<&Value>) -> T {
    value.and_then(Value::as_u64).and_then(|value| T::try_from(value).ok()).unwrap_or_default()
}
