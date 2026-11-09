//! The CLUT bank (entry type 0 in the palette arena band): colour rows the loader copies into
//! VRAM (`wiki: formats/graphics#palette`).

use psx_formats::clut::Palette;
use rebof3_emi::Entry;

use crate::error::Result;
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
