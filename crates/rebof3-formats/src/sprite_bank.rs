//! The character sprite bank, stored as a type-1 LZ77 blob or as a raw type-0 container
//! (`wiki: formats/character#container`).

use rebof3_emi::{TYPE_LZ77, lz77};

use crate::error::Result;
use crate::registry::{Derived, Description, EMI_EXTENSION, Format, RoundTrip, Selector, State};

/// Load arguments of the raw bank: the whole container and the five section positions
/// (`wiki: formats/character#load-arguments`).
const RAW_ADDRESSES: [u32; 6] = [0x8003_b800, 0x8003_b80c, 0x8005_680c, 0x8005_f80c, 0x8007_180c, 0x8007_a80c];

/// The LZ77-packed bank as a format-registry row.
pub static LZ77_BLOB: Description = Description {
    format: Format::Lz77Blob,
    name: "character sprite blob, LZ77 (type 1)",
    state: State::Specified,
    spec_state: "partial",
    codec_state: "decode-only (in the game's own decoder)",
    evidence: "src/slus/func_80164064.c decodes 65/65; no encoder in the repo",
    extension: "lz77",
    selector: Selector::Type(TYPE_LZ77),
    round_trip: RoundTrip::None,
    derive: Some(derive),
    open: None,
    kind: "character-bank",
};

/// The raw bank as a format-registry row.
pub static RAW_BLOB: Description = Description {
    format: Format::CharacterBlob,
    name: "character sprite blob, raw (type 0)",
    state: State::Specified,
    spec_state: "partial",
    codec_state: "none",
    evidence: "wiki: formats/character#container: raw section offsets and sections",
    extension: EMI_EXTENSION,
    selector: Selector::Load(&RAW_ADDRESSES),
    round_trip: RoundTrip::None,
    derive: None,
    open: None,
    kind: "character-bank",
};

/// The LZ77-decoded blob, as the game's own decoder produces it.
fn derive(_load_arg: u32, payload: &[u8]) -> Result<Vec<Derived>> {
    Ok(vec![Derived { role: "decoded", extension: "bin", data: lz77::decode(payload)? }])
}
