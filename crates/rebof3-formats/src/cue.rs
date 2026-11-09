//! The type-8 cue table.
//! (`wiki: formats/audio#emi-type-8-cue-records`). The records are read by the `emi` crate, which

use rebof3_emi::cue::CueTable;
use rebof3_emi::{Entry, TYPE_CUE};

use crate::error::Result;
use crate::identity::difference;
use crate::registry::{Derived, Description, Format, RoundTrip, Selector, State};

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
    derive: Some(derive),
    open: None,
    kind: "audio-cues",
};

/// Decodes the table and encodes it again; `Err` names the first difference.
fn round_trip(_entry: &Entry, payload: &[u8]) -> Result<(), String> {
    let table = CueTable::decode(payload).map_err(|error| error.to_string())?;
    difference(&table.encode(), payload)
}

/// The cue records as JSON.
/// definition of a cue record; its format is `wiki: formats/audio#emi-type-8-cue-records`.
fn derive(_load_arg: u32, payload: &[u8]) -> Result<Vec<Derived>> {
    let data = CueTable::decode(payload)?.json().into_bytes();
    Ok(vec![Derived { role: "json", extension: "json", data }])
}
