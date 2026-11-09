//! The CLUT block (entry type 0 in the palette arena band).
//! VRAM (`wiki: formats/graphics#palette`).

use psx_formats::clut::{self, Palette};
use psx_formats::gfx::texture::Clut;
use rebof3_emi::Entry;
use rebof3_emi::texture::{self as draw, ClutBlock as Placement};

use crate::error::{Error, Result};
use crate::identity::difference;
use crate::registry::{Derived, Description, Format, RoundTrip, Selector, State};

/// The load-address window that holds CLUT blocks.
const BAND: std::ops::RangeInclusive<u32> = 0x8003_3800..=0x8003_6e00;

/// The block as a format-registry row.
pub static BLOCK: Description = Description {
    format: Format::ClutBlock,
    name: "CLUT block (type 0 in the palette arena band)",
    state: State::Codec,
    spec_state: "documented",
    codec_state: "decode+encode",
    evidence: "wiki: formats/graphics#palette; 575 census palettes sit in the band",
    extension: "clut",
    selector: Selector::LoadWindow(BAND),
    round_trip: RoundTrip::Payload(round_trip),
    derive: Some(derive),
    open: None,
    kind: "palette",
};

/// Decodes the block and encodes it again; `Err` names the first difference.
fn round_trip(_entry: &Entry, payload: &[u8]) -> Result<(), String> {
    let palette = Palette::decode(payload).map_err(|error| error.to_string())?;
    difference(&palette.encode(), payload)
}

/// The palette as JSON and as a swatch strip PNG.
fn derive(_load_arg: u32, payload: &[u8]) -> Result<Vec<Derived>> {
    let palette = Palette::decode(payload)?;
    Ok(vec![
        Derived { role: "json", extension: "json", data: palette.json().into_bytes() },
        Derived { role: "png", extension: "png", data: palette.png_strip() },
    ])
}

/// A CLUT block: its color words, their RGBA view, and where the arena loads it in VRAM.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClutBlock {
    /// The color words in stored order.
    pub words: Vec<u16>,
    /// The RGBA view of `words` (see [`color_rgba`]).
    pub colors: Vec<[u8; 4]>,
    /// The VRAM position of the first color, when the block loads into the palette arena.
    pub vram: Option<Clut>,
}

/// The lossless RGBA view of a color word (`clut::word_to_view`).
pub fn color_rgba(word: u16) -> [u8; 4] {
    clut::word_to_view(word)
}

/// The CLUT block a type-0 payload loaded at `load_arg` holds.
pub fn clut_block(load_arg: u32, payload: &[u8]) -> Result<ClutBlock> {
    let words = clut::Palette::decode(payload)?.words().to_vec();
    let colors = words.iter().map(|&word| clut::word_to_view(word)).collect();
    let vram = Placement::new(0, load_arg, payload.len()).map(|block| draw::clut_at(block.arena_word));
    Ok(ClutBlock { words, colors, vram })
}

/// The JSON of a CLUT block, as the export writes it (`<slot>.json`).
pub fn export(payload: &[u8]) -> Result<Vec<u8>> {
    Ok(clut::Palette::decode(payload)?.json().into_bytes())
}

/// Reads a palette's JSON (`<slot>.json`) back to its stored bytes.
pub fn import(json: &[u8]) -> Result<Vec<u8>> {
    let document: serde_json::Value =
        serde_json::from_slice(json).map_err(|error| Error::Import(format!("palette JSON: {error}")))?;
    if document["format"] != "psx-clut" {
        return Err(Error::Import("the JSON is not a psx-clut palette".into()));
    }
    let words = document["colors"]
        .as_array()
        .and_then(|colors| {
            colors.iter().map(|color| color["word"].as_u64().and_then(|word| u16::try_from(word).ok())).collect()
        })
        .ok_or_else(|| Error::Import("each palette color needs a 16-bit word".into()))?;
    Ok(clut::Palette::from_words(words)?.encode())
}

/// `block` with `words` written over its color words from word `offset`:
pub fn splice(block: &[u8], offset: usize, words: &[u16]) -> Result<Vec<u8>> {
    let end = offset.checked_add(words.len()).filter(|end| end * 2 <= block.len()).ok_or_else(|| {
        Error::Import(format!(
            "{} color words at word {offset} run past the block's {} words",
            words.len(),
            block.len() / 2
        ))
    })?;
    let mut out = block.to_vec();
    for (slot, word) in out[offset * 2..end * 2].as_chunks_mut::<2>().0.iter_mut().zip(words) {
        *slot = word.to_le_bytes();
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_replaces_only_its_words() {
        let block: Vec<u8> = (0..32u16).flat_map(|word| word.to_le_bytes()).collect();
        let row: Vec<u16> = (100..116).collect();
        let spliced = splice(&block, 16, &row).unwrap();
        assert_eq!(&spliced[..32], &block[..32]);
        assert_eq!(u16::from_le_bytes([spliced[32], spliced[33]]), 100);
        let same: Vec<u16> = (16..32).collect();
        assert_eq!(splice(&block, 16, &same).unwrap(), block);
        assert!(splice(&block, 24, &row).is_err());
    }
}
