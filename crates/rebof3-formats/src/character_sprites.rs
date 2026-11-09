//! The character sprites, stored as a type-1 LZ77 blob or as a raw type-0 container
//! (`wiki: formats/character#container`).

use rebof3_emi::{TYPE_LZ77, lz77};

use crate::error::Result;
use crate::registry::{Derived, Description, EMI_EXTENSION, Format, RoundTrip, Selector, State};

/// Load arguments of the raw sprites: the whole container and the five section positions
/// (`wiki: formats/character#load-arguments`).
const RAW_ADDRESSES: [u32; 6] = [0x8003_b800, 0x8003_b80c, 0x8005_680c, 0x8005_f80c, 0x8007_180c, 0x8007_a80c];

/// The LZ77-packed sprites as a format-registry row.
pub static LZ77: Description = Description {
    format: Format::CharacterSpritesLz77,
    name: "character sprites, LZ77 (type 1)",
    state: State::Specified,
    spec_state: "partial",
    codec_state: "decode-only (in the game's own decoder)",
    evidence: "src/engine/slus/LZ77_DecodeStep.c decodes 65/65; no encoder in the repo",
    extension: "lz77",
    selector: Selector::Type(TYPE_LZ77),
    round_trip: RoundTrip::None,
    derive: Some(derive),
    open: None,
    kind: "character-sprites",
};

/// The raw sprites as a format-registry row.
pub static RAW: Description = Description {
    format: Format::CharacterSprites,
    name: "character sprites, raw (type 0)",
    state: State::Specified,
    spec_state: "partial",
    codec_state: "none",
    evidence: "wiki: formats/character#container: raw section offsets and sections",
    extension: EMI_EXTENSION,
    selector: Selector::Load(&RAW_ADDRESSES),
    round_trip: RoundTrip::None,
    derive: None,
    open: None,
    kind: "character-sprites",
};

/// The LZ77-decoded blob, as the game's own decoder produces it.
fn derive(_load_arg: u32, payload: &[u8]) -> Result<Vec<Derived>> {
    Ok(vec![Derived { role: "decoded", extension: "bin", data: lz77::decode(payload)? }])
}
