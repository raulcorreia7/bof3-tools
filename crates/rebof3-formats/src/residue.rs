//! The entries that are not an asset format: code images, reserved slots, and what no rule
//! classifies.

use crate::registry::{Description, EMI_EXTENSION, Format, RoundTrip, Selector, State};

/// Load addresses that hold code images: the EMI code targets of `config/emi/**/target.toml`, plus
/// the two addresses the asset census's code classifier found without a configured target.
const CODE_ADDRESSES: [u32; 9] = [
    0x8009_6800, // emi/battle/battle/15
    0x800c_1800,
    0x800f_5000, // emi/world00/area030/05, the battle code base
    0x8019_5800, // emi/etc/game/00 (GAME.EMI) and its inline tables
    0x801c_e400,
    0x801d_0c00, // emi/world00/area030/04, emi/battle/battle/03, the etc/ overlays
    0x801e_ec00, // emi/etc/commu00/00
    0x801f_2c00, // the area program code arena
    0x801f_6c00, // the arena's second page
];

/// The code images as a format-registry row.
pub static CODE: Description = Description {
    format: Format::CodeImage,
    name: "code image (configured EMI code targets)",
    state: State::Code,
    spec_state: "n/a",
    codec_state: "n/a",
    evidence: "config/emi/**/target.toml load addresses; disc-census code rows",
    extension: EMI_EXTENSION,
    selector: Selector::Load(&CODE_ADDRESSES),
    round_trip: RoundTrip::None,
};

/// The reserved slots as a format-registry row; `classify` selects them by their all-zero payload.
pub static RESERVE: Description = Description {
    format: Format::Reserve,
    name: "reserve (all-zero payload)",
    state: State::Reserve,
    spec_state: "n/a",
    codec_state: "n/a",
    evidence: "all-zero payload; the loader reserves the destination",
    extension: EMI_EXTENSION,
    selector: Selector::Fallback,
    round_trip: RoundTrip::None,
};

/// The entries no rule matched, as a format-registry row.
pub static UNCLASSIFIED: Description = Description {
    format: Format::Unclassified,
    name: "unclassified",
    state: State::Unknown,
    spec_state: "none",
    codec_state: "none",
    evidence: "no selection rule matched",
    extension: EMI_EXTENSION,
    selector: Selector::Fallback,
    round_trip: RoundTrip::None,
};
