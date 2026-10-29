//! Where a text file's strings sit: the disc path, the EMI entry, the byte in that entry, the disc
//! offset and the address the payload loads at.
//!
//! One [`PayloadSource`] describes a whole payload; [`PayloadSource::origin`] turns a string's offset
//! inside that payload into the [`Origin`] a JSON row carries. The output directory holds one payload
//! per EMI slot, so the source chain is the same for every row of its JSON.

use serde::{Deserialize, Serialize};

/// One string's place in the disc: the file, the EMI entry, the byte offset inside that entry, and the
/// disc and RAM addresses when they are known.
///
/// `source` and `entry` are absent for a standalone `assets export strings` document, which reads a
/// payload file and knows no archive. `offset` is always the string's byte offset inside the payload.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Origin {
    /// Disc path of the EMI archive, with forward slashes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Index of the entry in the archive's table.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry: Option<usize>,
    /// Byte offset of the string inside the entry payload.
    #[serde(alias = "at")]
    pub offset: usize,
    /// Byte offset of the string inside the archive file: the entry's offset plus [`Self::offset`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_offset: Option<u64>,
    /// Logical byte offset on the disc: the archive's LBA times 2048, plus [`Self::file_offset`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disc_offset: Option<u64>,
    /// First sector of the archive file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lba: Option<u32>,
    /// The string's runtime address: the payload's load address plus [`Self::offset`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ram: Option<u32>,
}

impl Origin {
    /// The origin of the string at `offset` in a payload with no archive around it.
    pub fn plain(offset: usize) -> Self {
        Self { offset, ..Self::default() }
    }
}

/// One payload's source chain, from which each string's [`Origin`] is made.
#[derive(Clone, Debug)]
pub struct PayloadSource {
    /// Disc path of the archive.
    pub source: String,
    /// Entry index in the archive.
    pub entry: usize,
    /// Offset of the entry's first byte in the archive file.
    pub entry_offset: u64,
    /// First sector of the archive file, when the disc image was read.
    pub lba: Option<u32>,
    /// Address the payload loads at, when the entry names one.
    pub load: Option<u32>,
}

impl PayloadSource {
    /// The origin of the string at `offset` in the entry payload.
    pub fn origin(&self, offset: usize) -> Origin {
        let file_offset = self.entry_offset + offset as u64;
        Origin {
            source: Some(self.source.clone()),
            entry: Some(self.entry),
            offset,
            file_offset: Some(file_offset),
            disc_offset: self.lba.map(|lba| u64::from(lba) * 2048 + file_offset),
            lba: self.lba,
            ram: self.load.map(|load| load + offset as u32),
        }
    }
}
