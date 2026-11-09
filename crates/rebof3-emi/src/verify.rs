//! The container round trip: parse a container, repack its payloads, and compare the bytes with
//! the original.

use std::fs;
use std::path::Path;

use crate::emi::{Archive, archives_under};
use crate::error::Result;
use crate::pack::{Payload, pack_payloads};

/// The container round trip over a set of archives.
#[derive(Clone, Debug, Default)]
pub struct Container {
    /// Archives checked.
    pub archives: usize,
    /// Entries in them.
    pub entries: usize,
    /// Payload bytes in them.
    pub bytes: u64,
    /// One line per archive that did not repack byte-exactly.
    pub failures: Vec<String>,
}

impl Container {
    /// Repacks one container's payloads and compares the bytes with the original. `label` names it
    /// in failures. Returns the parsed table when the container opens.
    pub fn check(&mut self, label: &str, bytes: &[u8]) -> Option<Archive> {
        self.archives += 1;
        let archive = match Archive::parse(bytes) {
            Ok(archive) => archive,
            Err(error) => {
                self.failures.push(format!("{label}: {error}"));
                return None;
            }
        };
        self.entries += archive.entries().len();
        self.bytes += archive.entries().iter().map(|entry| u64::from(entry.size)).sum::<u64>();

        let payloads: Result<Vec<Payload>> =
            archive.entries().iter().enumerate().map(|(index, entry)| Ok(Payload { bytes: archive.payload(bytes, index)?, file_type: entry.file_type, ram_ptr: entry.ram_ptr })).collect();
        let packed = payloads.and_then(|payloads| pack_payloads(archive.version(), &payloads));
        match packed {
            Ok(packed) => {
                if let Some(offset) = first_difference(bytes, &packed) {
                    self.failures.push(format!("{label}: differs at 0x{offset:X} (retail {} bytes, repacked {} bytes)", bytes.len(), packed.len()));
                }
            }
            Err(error) => self.failures.push(format!("{label}: pack failed: {error}")),
        }
        Some(archive)
    }
}

/// Repacks every archive under `root` from its parsed table and compares the bytes with the
/// retail container.
pub fn container_round_trip(root: &Path) -> Result<Container> {
    let mut container = Container::default();
    for path in archives_under(root) {
        container.check(&path.display().to_string(), &fs::read(&path)?);
    }
    Ok(container)
}

/// First offset where two byte runs differ, or `None` when they are equal.
pub fn first_difference(left: &[u8], right: &[u8]) -> Option<usize> {
    let common = left.len().min(right.len());
    for index in 0..common {
        if left[index] != right[index] {
            return Some(index);
        }
    }
    (left.len() != right.len()).then_some(common)
}
