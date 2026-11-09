//! The area message table (`0x80010000`): the strings a scene shows (`wiki: formats/area`).

pub mod po;
pub mod table;

use crate::registry::{Description, EMI_EXTENSION, Format, RoundTrip, Selector, State};

/// The load address of the table.
const LOAD_ADDRESS: u32 = 0x8001_0000;

/// The table as a format-registry row.
pub static TABLE: Description = Description {
    format: Format::AreaMessageTable,
    name: "area message table (0x80010000)",
    state: State::Specified,
    spec_state: "documented",
    codec_state: "none",
    evidence: "wiki: formats/area (`0x80010000`)",
    extension: EMI_EXTENSION,
    selector: Selector::Load(&[LOAD_ADDRESS]),
    round_trip: RoundTrip::None,
    derive: None,
    open: None,
    kind: "area-message",
};
