//! The shared resource table (`0x8001A000`): references the text refers to
//! (`wiki: data/text#references`).

use crate::registry::{Description, EMI_EXTENSION, Format, RoundTrip, Selector, State};

/// The load address of the table.
const LOAD_ADDRESS: u32 = 0x8001_a000;

/// The table as a format-registry row.
pub static SHARED: Description = Description {
    format: Format::SharedResourceTable,
    name: "shared resource table (0x8001A000)",
    state: State::Specified,
    spec_state: "documented",
    codec_state: "none",
    evidence: "wiki: data/text#references",
    extension: EMI_EXTENSION,
    selector: Selector::Load(&[LOAD_ADDRESS]),
    round_trip: RoundTrip::None,
};
