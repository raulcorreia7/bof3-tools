//! The EMI container table from the `emi` crate, bound to this crate's error so
//! game code has one error type to match on.

use crate::error::Result;

#[cfg(feature = "std")]
use std::io::{Read, Seek};
#[cfg(feature = "std")]
use std::path::Path;

pub use ::emi::{Entry, HEADER_SIZE, MAGIC, SECTOR_SIZE, TOC_ENTRY_SIZE, TYPE_CUE, TYPE_LZ77, align_sector};
#[cfg(feature = "std")]
pub use ::emi::{TYPE_VRAM, ensure_parent, entry_name, type_extension};

/// A parsed container table; see [`::emi::Archive`].
#[derive(Debug)]
pub struct Archive(::emi::Archive);

impl Archive {
    /// Reads and validates the table of a container held in memory.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        Ok(Self(::emi::Archive::parse(bytes)?))
    }

    /// The table's version word.
    pub fn version(&self) -> u32 {
        self.0.version()
    }

    /// The table entries, in table order.
    pub fn entries(&self) -> &[Entry] {
        self.0.entries()
    }

    /// The payload of entry `index` inside `container`, the bytes this table was
    /// parsed from.
    pub fn payload<'a>(&self, container: &'a [u8], index: usize) -> Result<&'a [u8]> {
        Ok(self.0.payload(container, index)?)
    }
}

#[cfg(feature = "std")]
impl Archive {
    /// Reads and validates the table of the container at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self(::emi::Archive::open(path)?))
    }

    /// Reads and validates the table of a container read from `reader`; only the
    /// first sector is read. Payloads are read later with [`Archive::read_payload`].
    pub fn from_reader<R: Read + Seek>(reader: &mut R) -> Result<Self> {
        Ok(Self(::emi::Archive::from_reader(reader)?))
    }

    /// The payload of entry `index`, read from `reader`: the stream this table was
    /// read from.
    pub fn read_payload<R: Read + Seek>(&self, reader: &mut R, index: usize) -> Result<Vec<u8>> {
        Ok(self.0.read_payload(reader, index)?)
    }

    /// Writes entry `index` to `path`, creating parent directories.
    pub fn extract_entry(&self, index: usize, path: impl AsRef<Path>) -> Result<()> {
        Ok(self.0.extract_entry(index, path)?)
    }

    /// The `emi.json` text for this container.
    pub fn manifest_json(&self, typed: bool) -> String {
        crate::manifest::to_json(self.version(), self.entries(), typed)
    }

    /// Writes every entry into `directory` as `<index>.<ext>`.
    pub fn extract_all(&self, directory: impl AsRef<Path>, typed: bool) -> Result<()> {
        Ok(self.0.extract_all(directory, typed)?)
    }
}

/// The payload bytes of one entry, read into memory.
#[cfg(feature = "std")]
pub fn read_entry(archive_path: impl AsRef<Path>, index: usize) -> Result<Vec<u8>> {
    Ok(::emi::read_entry(archive_path, index)?)
}
