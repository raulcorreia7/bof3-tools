//! The import direction: the open formats back to the game's own bytes.

use crate::error::Result;

/// The characters of `ETC/START.EMI` entry 8 from the JSON of `tables::characters`, written over the stored payload.
pub fn characters(json: &[u8], payload: &[u8]) -> Result<Vec<u8>> {
    Ok(rebof3_formats::characters::import(json, payload)?)
}

/// The palettes of one character sprites from the JSON of `sprites::character_sprites`, written over the decoded sprites.
pub fn character_sprites(json: &[u8], payload: &[u8]) -> Result<Vec<u8>> {
    Ok(rebof3_formats::sprite_palettes::import(json, payload)?)
}

/// An EMI container with the entries named in `replace` swapped for new payloads.
pub fn emi(container: &[u8], replace: &[(usize, &[u8])]) -> Result<Vec<u8>> {
    Ok(rebof3_emi::pack::rebuild(container, replace)?)
}
