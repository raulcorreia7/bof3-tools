//! The auxiliary module window (`0x80117000`).
//! The auxiliary module window (`0x80117000`): a module id word, then code or data
//! (`wiki: formats/area`).

use crate::registry::{Description, EMI_EXTENSION, Format, RoundTrip, Selector, State};

/// The load address of the window.
const LOAD_ADDRESS: u32 = 0x8011_7000;

/// The window as a format-registry row.
pub static AUX: Description = Description {
    format: Format::AreaAuxWindow,
    name: "area auxiliary module window (0x80117000)",
    state: State::Specified,
    spec_state: "partial",
    codec_state: "none",
    evidence: "wiki: formats/area (`0x80117000`); a module id word, then code (area049) or data",
    extension: EMI_EXTENSION,
    selector: Selector::Load(&[LOAD_ADDRESS]),
    round_trip: RoundTrip::None,
    derive: None,
    open: None,
    kind: "area-module",
};
