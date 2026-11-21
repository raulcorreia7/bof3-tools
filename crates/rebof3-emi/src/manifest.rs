//! The `emi.json` manifest an extraction writes next to the payloads and a
//! pack reads back (`wiki: formats/emi`).

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
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<&'static str>,
    index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<&'static str>,
    name: String,
    ram_ptr: u32,
    size: u32,
    #[serde(rename = "type")]
    file_type: u16,
}

/// What a format registry says an entry is: its format name and asset kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Label {
    /// The registry's format name.
    pub format: &'static str,
    /// The asset kind the format's files are written under.
    pub kind: &'static str,
}

/// The manifest text for a container with `entries`; `labels` holds one label per entry, or is empty.
pub fn to_json(archive_version: u32, entries: &[Entry], typed: bool, labels: &[Label]) -> String {
    let manifest = Manifest {
        archive_version,
        entries: entries
            .iter()
            .enumerate()
            .map(|(index, entry)| ManifestEntry {
                first4: entry.first4,
                format: labels.get(index).map(|label| label.format),
                index,
                kind: labels.get(index).map(|label| label.kind),
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

/// The entries a manifest names, and its container version.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedManifest {
    /// The container's `archive_version`; `0` when the manifest has none.
    pub archive_version: u32,
    /// The named entries in manifest order. Rows without a non-empty `name` are metadata rows: the
    /// extractor writes them, and they are never packed.
    pub entries: Vec<ManifestInput>,
}

/// One named entry of a manifest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManifestInput {
    /// The file name, relative to the manifest's folder.
    pub name: String,
    /// The EMI entry type.
    pub file_type: u16,
    /// The load address.
    pub ram_ptr: u32,
}

/// Reads manifest text: its named entries and `archive_version`.
pub fn parse_manifest(bytes: &[u8]) -> Result<ParsedManifest> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| Error::InvalidArchive("invalid manifest JSON"))?;
    let rows = value.get("entries").and_then(Value::as_array).ok_or(Error::InvalidArchive("manifest entries must be an array"))?;
    let entries: Vec<ManifestInput> = rows
        .iter()
        .filter_map(|row| {
            row.get("name").and_then(Value::as_str).filter(|name| !name.is_empty()).map(|name| ManifestInput {
                name: name.to_owned(),
                file_type: json_number(row.get("type")),
                ram_ptr: json_number(row.get("ram_ptr")),
            })
        })
        .collect();
    if entries.is_empty() {
        return Err(Error::InvalidArchive("manifest has no named entries"));
    }
    Ok(ParsedManifest { archive_version: json_number(value.get("archive_version")), entries })
}

/// Loads a manifest's named entries and validates every one of them against
/// `folder` before any payload is read or any output is created.
///
/// Returns the entries in manifest order, with the canonical path of the
/// validated regular file each name resolves to, plus the manifest's
/// `archive_version`.
///
/// A name is rejected when it is absolute, when it contains a parent-directory
/// component, when it is missing or not a regular file, or when its resolved
/// path leaves the canonical input folder (a symlink escape).
pub fn manifest_entries(folder: impl AsRef<Path>, manifest_path: impl AsRef<Path>) -> Result<(Vec<PackEntry>, u32)> {
    let manifest = parse_manifest(&fs::read(manifest_path.as_ref())?)?;
    let root = canonical_folder(folder.as_ref())?;
    let inputs = manifest.entries.iter().map(|entry| Ok(PackEntry { path: manifest_input_path(&root, &entry.name)?, file_type: entry.file_type, ram_ptr: entry.ram_ptr })).collect::<Result<_>>()?;
    Ok((inputs, manifest.archive_version))
}

/// The canonical input folder: the containment root every manifest name is
/// compared against as path components.
fn canonical_folder(folder: &Path) -> Result<PathBuf> {
    let root = fs::canonicalize(folder).map_err(|error| Error::ManifestInput(format!("input folder {}: {error}", folder.display())))?;
    if !root.is_dir() {
        return Err(Error::ManifestInput(format!("input folder {} is not a directory", folder.display())));
    }
    Ok(root)
}

/// Resolves one manifest name inside `root`, rejecting every name that could
/// read or delete a file the caller did not put in the input folder.
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
    let resolved = fs::canonicalize(&candidate).map_err(|error| Error::ManifestInput(format!("{}: {error}", candidate.display())))?;
    if !resolved.starts_with(root) {
        return Err(Error::ManifestInput(format!("manifest entry {name:?} resolves outside the input folder: {}", resolved.display())));
    }
    if !fs::metadata(&resolved)?.is_file() {
        return Err(Error::ManifestInput(format!("manifest entry {name:?} is not a regular file: {}", resolved.display())));
    }
    Ok(resolved)
}

/// A JSON number as the integer type the manifest field holds, or `0`.
fn json_number<T: TryFrom<u64> + Default>(value: Option<&Value>) -> T {
    value.and_then(Value::as_u64).and_then(|value| T::try_from(value).ok()).unwrap_or_default()
}
