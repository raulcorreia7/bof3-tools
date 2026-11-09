//! The area message bank (`0x80010000`): the strings a scene shows (`wiki: formats/area`).

use crate::registry::{Description, EMI_EXTENSION, Format, RoundTrip, Selector, State};

/// The load address of the bank.
const LOAD_ADDRESS: u32 = 0x8001_0000;

/// The bank as a format-registry row.
pub static BANK: Description = Description {
    format: Format::AreaMessageBank,
    name: "area message bank (0x80010000)",
    state: State::Specified,
    spec_state: "documented",
    codec_state: "none",
    evidence: "wiki: formats/area (`0x80010000`)",
    extension: EMI_EXTENSION,
    selector: Selector::Load(&[LOAD_ADDRESS]),
    round_trip: RoundTrip::None,
    derive: None,
    kind: "area-message",
};
