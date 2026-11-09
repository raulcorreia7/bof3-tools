//! The packed 4-bit cell grid (`0x80032000`) of an area (`wiki: formats/area`).

use crate::registry::{Description, EMI_EXTENSION, Format, RoundTrip, Selector, State};

/// The load address of the grid.
const LOAD_ADDRESS: u32 = 0x8003_2000;

/// The grid as a format-registry row.
pub static GRID: Description = Description {
    format: Format::AreaCellGrid,
    name: "area packed 4-bit cell grid (0x80032000)",
    state: State::Specified,
    spec_state: "documented",
    codec_state: "none",
    evidence: "wiki: formats/area (`0x80032000`); reader func_801C6FD4",
    extension: EMI_EXTENSION,
    selector: Selector::Load(&[LOAD_ADDRESS]),
    round_trip: RoundTrip::None,
    derive: None,
};
