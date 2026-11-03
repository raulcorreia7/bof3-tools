//! Sprite banks: the palettes of a character bank as JSON.
//!
//! The typed layer lives in `rebof3_emi::character_bank`, which this module re-exports. A bank's node data is
//! not decoded yet, so [`character_bank`] writes the palettes of each section and nothing else.
//!
//! # Cross-reference
//!
//! | Family | C record (`decomp/include`) |
//! | --- | --- |
//! | sprite bank | none yet (no reader proves a runtime record) |
//!
//! The layout is in `wiki: formats/character`.
//!
//! The full map, with the field names that differ, is
//! `wiki: formats/conversion#c-records-rust-decoders-and-the-layout`: the Rust `Section.start` is the
//! section's node count and `Section.table` its node offset table.

use crate::Result;

pub use rebof3_emi::character_bank::{Bank, parse};

/// The JSON of the palettes of one sprite bank. `payload` is the decoded bank: a whole container, or one section.
pub fn character_bank(payload: &[u8]) -> Result<Vec<u8>> {
    Ok(rebof3_emi::character_bank::export(payload)?)
}
