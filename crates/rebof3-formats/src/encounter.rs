//! The encounters and enemy records (`0x800E4000`) of an area (`wiki: formats/area`).

use crate::registry::{Description, EMI_EXTENSION, Format, RoundTrip, Selector, State};

/// The load address of the block.
const LOAD_ADDRESS: u32 = 0x800e_4000;

/// The block as a format-registry row.
pub static TABLE: Description = Description {
    format: Format::AreaEncounterTable,
    name: "area encounters and enemy records (0x800E4000)",
    state: State::Specified,
    spec_state: "documented",
    codec_state: "none",
    evidence: "wiki: formats/area (`0x800E4000`); include/game/enemy.h models it",
    extension: EMI_EXTENSION,
    selector: Selector::Load(&[LOAD_ADDRESS]),
    round_trip: RoundTrip::None,
};
