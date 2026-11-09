//! The type-8 cue table: one record per sound cue of a bank
//! (`wiki: formats/audio#emi-type-8-cue-records`). The records are read by the `emi` crate, which
//! the audio library also uses; this module binds them to the registry.

use rebof3_emi::cue::CueTable;
use rebof3_emi::{Entry, TYPE_CUE};

use crate::identity::difference;
use crate::registry::{Description, Format, RoundTrip, Selector, State};

/// The table as a format-registry row.
pub static TABLE: Description = Description {
    format: Format::CueTable,
    name: "type-8 cue records (type 8)",
    state: State::Codec,
    spec_state: "documented",
    codec_state: "decode+encode",
    evidence: "wiki: formats/audio#emi-type-8-cue-records",
    extension: "emi8",
    selector: Selector::Type(TYPE_CUE),
    round_trip: RoundTrip::Payload(round_trip),
};

/// Decodes the table and encodes it again; `Err` names the first difference.
fn round_trip(_entry: &Entry, payload: &[u8]) -> Result<(), String> {
    let table = CueTable::decode(payload).map_err(|error| error.to_string())?;
    difference(&table.encode(), payload)
}
