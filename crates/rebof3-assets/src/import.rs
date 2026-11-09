//! The import direction: the open formats back to the game's own bytes.
//!
//! Each import reads what its export wrote and returns the stored bytes of the
//! entry. A texture PNG is either the 16 bpp view (`<slot>.png`) or an indexed
//! row (`<slot>.clut-<x>-<y>.png`), and [`texture`] tells them apart by the PNG's
//! color type. The owner's disc round-trips byte for byte (`tests/round_trip.rs`).

use psx_formats::adpcm;
use psx_formats::gfx::texture::{self, Depth};
use psx_formats::gfx::{clut, png, vram};

use crate::decode::VAG_RATE;
use crate::error::{Error, Result};

/// The characters of `ETC/START.EMI` entry 8 from the JSON of `tables::characters`, written over the stored payload.
pub fn characters(json: &[u8], payload: &[u8]) -> Result<Vec<u8>> {
    Ok(rebof3_emi::characters::import(json, payload)?)
}

/// The palettes of one sprite bank from the JSON of `sprites::character_bank`, written over the decoded bank.
pub fn character_bank(json: &[u8], payload: &[u8]) -> Result<Vec<u8>> {
    Ok(rebof3_emi::character_bank::import(json, payload)?)
}

/// A sequence package (`.seq`) from Standard MIDI Files, one sequence per part and in order. Each part is a MIDI
/// file that [`crate::decode::sequence_midi`] writes, with the sequence id the package gives it.
pub fn sequence(parts: &[(&[u8], u16)]) -> Result<Vec<u8>> {
    let sequences = parts
        .iter()
        .map(|&(midi, seq_id)| psx_formats::sep::Sequence::from_midi(midi, seq_id))
        .collect::<psx_formats::Result<Vec<_>>>()?;
    Ok(psx_formats::sep::Package { sequences }.encode())
}

/// An EMI container with the entries named in `replace` swapped for new payloads. Every other entry keeps its
/// stored payload, type and load address, and the container keeps its version. With no replacement the result is
/// the container's bytes.
pub fn emi(container: &[u8], replace: &[(usize, &[u8])]) -> Result<Vec<u8>> {
    Ok(rebof3_emi::pack::rebuild(container, replace)?)
}

/// Encodes a WAVE file as a VAG: its samples as SPU ADPCM blocks. The file must be mono 16-bit PCM at
/// [`VAG_RATE`], which is what [`crate::decode::pcm_wav`] writes. ADPCM cannot reproduce every sample, so the
/// first encode is lossy for those; a second encode of its decoded samples is exact.
pub fn vag(wav: &[u8]) -> Result<Vec<u8>> {
    Ok(adpcm::encode_stream(&read_pcm_wav(wav)?))
}

/// The samples of a mono 16-bit PCM WAVE file at [`VAG_RATE`].
fn read_pcm_wav(bytes: &[u8]) -> Result<Vec<i16>> {
    let bad = |reason: &str| Error::Import(format!("the WAVE file {reason}"));
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(bad("is not a RIFF WAVE file"));
    }
    let (mut format, mut data) = (None, None);
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let size = u32::from_le_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]]) as usize;
        let body = bytes.get(at + 8..at + 8 + size).ok_or_else(|| bad("has a chunk past its end"))?;
        match &bytes[at..at + 4] {
            b"fmt " => format = Some(body),
            b"data" => data = Some(body),
            _ => {}
        }
        at += 8 + size + (size & 1);
    }
    let format = format.filter(|format| format.len() >= 16).ok_or_else(|| bad("has no format chunk"))?;
    let data = data.ok_or_else(|| bad("has no data chunk"))?;
    let tag = u16::from_le_bytes([format[0], format[1]]);
    let channels = u16::from_le_bytes([format[2], format[3]]);
    let rate = u32::from_le_bytes([format[4], format[5], format[6], format[7]]);
    let bits = u16::from_le_bytes([format[14], format[15]]);
    if tag != 1 || channels != 1 || rate != VAG_RATE || bits != 16 {
        return Err(bad("must be mono 16-bit PCM at 44100 Hz"));
    }
    Ok(data.as_chunks::<2>().0.iter().map(|pair| i16::from_le_bytes(*pair)).collect())
}

/// Reads a palette's JSON (`<slot>.json`) back to its stored bytes: the `word` of each color, in order.
/// The `rgba` and `stp` fields are views of the word and are not read.
pub fn palette(json: &[u8]) -> Result<Vec<u8>> {
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

/// A texture read back from an indexed row: its payload and the CLUT row its
/// texels are drawn with.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexedTexture {
    /// The type-3 payload, as stored.
    pub payload: Vec<u8>,
    /// The CLUT row's color words, as stored.
    pub palette: Vec<u16>,
    /// The texel depth, from the PNG's bit depth.
    pub depth: Depth,
}

/// What a texture PNG holds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TextureImport {
    /// A 16 bpp view: the payload, as stored.
    Words(Vec<u8>),
    /// An indexed row: the payload and the CLUT row it is drawn with.
    Indexed(IndexedTexture),
}

/// Reads a texture PNG that the export wrote, as its stored payload.
///
/// A view returns every word exactly, STP included. An indexed row packs its
/// texels back into words of its depth, and its colors return as color words.
pub fn texture(bytes: &[u8]) -> Result<TextureImport> {
    Ok(match png::read(bytes)? {
        png::Picture::Rgba(picture) => {
            let words: Vec<u16> =
                picture.pixels.as_chunks::<4>().0.iter().map(|&pixel| clut::view_to_word(pixel)).collect();
            let payload = vram::payload_from_grid(picture.width as usize, picture.height as usize, &words)?;
            TextureImport::Words(payload)
        }
        png::Picture::Indexed(picture) => {
            let depth = if picture.bits == 4 { Depth::Four } else { Depth::Eight };
            let (width, height) = (picture.width as usize, picture.height as usize);
            let per_word = depth.texels_per_word();
            if width % per_word != 0 {
                let words = picture.indices.len();
                return Err(psx_formats::Error::VramGridShape { width, height, words }.into());
            }
            let words = texture::texel_words(&picture.indices, depth);
            let payload = vram::payload_from_grid(width / per_word, height, &words)?;
            let palette = picture.colors.iter().map(|&color| clut::view_to_word(color)).collect();
            TextureImport::Indexed(IndexedTexture { payload, palette, depth })
        }
    })
}
