//! The party characters table of `ETC/START.EMI` entry 8 as JSON.

use crate::Result;

/// The eight playable characters of `ETC/START.EMI` entry 8 as JSON: each name, and the rest of its base-stat record as hex.
pub fn characters(payload: &[u8]) -> Result<Vec<u8>> {
    Ok(rebof3_formats::characters::export(payload)?)
}
