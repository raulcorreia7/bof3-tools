//! An archive of the disc and the key of an entry in it.

use std::sync::Arc;

use rebof3_emi::{Archive, Entry};
use rebof3_formats::{Format, classify};

/// One archive of the disc: its path relative to the disc folder, with `/` separators.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ArchivePath(pub String);

impl ArchivePath {
    /// The folder part (`WORLD02`) and the file name (`AREA105.EMI`).
    pub fn split(&self) -> (&str, &str) {
        self.0.rsplit_once('/').unwrap_or(("", &self.0))
    }
}

/// An edit's key: the archive and the entry index.
pub type Key = (ArchivePath, usize);

/// An archive read from the disc, with each entry's format.
pub struct Loaded {
    pub path: ArchivePath,
    pub bytes: Arc<Vec<u8>>,
    pub archive: Archive,
    pub formats: Vec<Format>,
}

impl Loaded {
    /// Reads an archive from its bytes and classifies each entry.
    pub fn parse(path: ArchivePath, bytes: Vec<u8>) -> Result<Self, rebof3_emi::Error> {
        let archive = Archive::parse(&bytes)?;
        let formats = (0..archive.entries().len()).map(|index| classify(&archive.entries()[index], archive.payload(&bytes, index).unwrap_or(&[]))).collect();
        Ok(Self { path, bytes: Arc::new(bytes), archive, formats })
    }

    pub fn entries(&self) -> &[Entry] {
        self.archive.entries()
    }

    /// The payload the disc stores for entry `index`.
    pub fn original(&self, index: usize) -> &[u8] {
        self.archive.payload(&self.bytes, index).unwrap_or(&[])
    }
}
