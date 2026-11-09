//! The import direction: the open formats back to the game's own bytes.
//!
//! Each import reads what its export wrote and returns the stored bytes of the
//! entry. The palette and texture imports live with their formats
//! (`rebof3_formats::clut::import`, `rebof3_formats::vram::import`). The owner's disc round-trips byte for byte (`tests/round_trip.rs`).

use crate::error::Result;

/// The characters of `ETC/START.EMI` entry 8 from the JSON of `tables::characters`, written over the stored payload.
pub fn characters(json: &[u8], payload: &[u8]) -> Result<Vec<u8>> {
    Ok(rebof3_formats::characters::import(json, payload)?)
}

/// The palettes of one character sprites from the JSON of `sprites::character_sprites`, written over the decoded sprites.
pub fn character_sprites(json: &[u8], payload: &[u8]) -> Result<Vec<u8>> {
    Ok(rebof3_formats::sprite_palettes::import(json, payload)?)
}

/// An EMI container with the entries named in `replace` swapped for new payloads. Every other entry keeps its
/// stored payload, type and load address, and the container keeps its version. With no replacement the result is
/// the container's bytes.
pub fn emi(container: &[u8], replace: &[(usize, &[u8])]) -> Result<Vec<u8>> {
    Ok(rebof3_emi::pack::rebuild(container, replace)?)
}
