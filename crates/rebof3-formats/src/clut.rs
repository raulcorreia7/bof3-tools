//! The CLUT bank (entry type 0 in the palette arena band): colour rows the loader copies into
//! VRAM (`wiki: formats/graphics#palette`).

use psx_formats::clut::{self, Palette};
use psx_formats::gfx::texture::Clut;
use rebof3_emi::Entry;
use rebof3_emi::texture::{self as draw, Bank};

use crate::error::{Error, Result};
use crate::identity::difference;
use crate::registry::{Derived, Description, Format, RoundTrip, Selector, State};

/// The load-address window that holds CLUT banks.
const BAND: std::ops::RangeInclusive<u32> = 0x8003_3800..=0x8003_6e00;

/// The bank as a format-registry row.
pub static BANK: Description = Description {
    format: Format::ClutBank,
    name: "CLUT bank (type 0 in the palette arena band)",
    state: State::Codec,
    spec_state: "documented",
    codec_state: "decode+encode",
    evidence: "wiki: formats/graphics#palette; 575 census palettes sit in the band",
    extension: "clut",
    selector: Selector::LoadWindow(BAND),
    round_trip: RoundTrip::Payload(round_trip),
    derive: Some(derive),
    kind: "palette",
};

/// Decodes the bank and encodes it again; `Err` names the first difference.
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

/// A palette bank: its color words, their RGBA view, and where the arena loads it in VRAM.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaletteBank {
    /// The color words in stored order.
    pub words: Vec<u16>,
    /// The RGBA view of `words` (see [`color_rgba`]).
    pub colors: Vec<[u8; 4]>,
    /// The VRAM position of the first color, when the bank loads into the palette arena.
    pub vram: Option<Clut>,
}

/// The lossless RGBA view of a color word (`clut::word_to_view`): `0x0000` reads as transparent and
/// the STP bit reads as half alpha, so [`crate::import`] returns the word exactly.
pub fn color_rgba(word: u16) -> [u8; 4] {
    clut::word_to_view(word)
}

/// The palette bank a type-0 payload loaded at `load_arg` holds.
pub fn palette_bank(load_arg: u32, payload: &[u8]) -> Result<PaletteBank> {
    let words = clut::Palette::decode(payload)?.words().to_vec();
    let colors = words.iter().map(|&word| clut::word_to_view(word)).collect();
    let vram = Bank::new(0, load_arg, payload.len()).map(|bank| draw::clut_at(bank.arena_word));
    Ok(PaletteBank { words, colors, vram })
}

/// The JSON of a palette bank, as the export writes it (`<slot>.json`).
pub fn export(payload: &[u8]) -> Result<Vec<u8>> {
    Ok(clut::Palette::decode(payload)?.json().into_bytes())
}

/// Reads a palette's JSON (`<slot>.json`) back to its stored bytes: the `word` of each color, in order.
/// The `rgba` and `stp` fields are views of the word and are not read.
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
