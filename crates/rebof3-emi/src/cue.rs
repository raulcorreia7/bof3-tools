//! The type-8 cue table from the `emi` crate, bound to this crate's error type.

use alloc::vec::Vec;

use crate::error::Result;

pub use ::emi::cue::{CHUNK_RECORDS, Cue, RECORD_BYTES};

/// One bank's cue records, in record-index order; see [`::emi::cue::CueTable`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CueTable(::emi::cue::CueTable);

impl CueTable {
    /// Reads a type-8 payload.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Ok(Self(::emi::cue::CueTable::decode(bytes)?))
    }

    /// The records, in cue-id order.
    pub fn records(&self) -> &[Cue] {
        self.0.records()
    }

    /// The record at `index`, if the table holds one.
    pub fn record(&self, index: usize) -> Option<&Cue> {
        self.0.record(index)
    }

    /// The records as JSON, one object per cue in cue-id order.
    #[cfg(feature = "std")]
    pub fn json(&self) -> String {
        self.0.json()
    }

    /// The payload bytes.
    pub fn encode(&self) -> Vec<u8> {
        self.0.encode()
    }
}
