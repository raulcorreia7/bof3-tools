//! The VAB bank (entry types 6 and 7): the header with its programs and tones, and the body of
//! ADPCM samples that shares the header's load argument (`wiki: formats/audio#bank-layout`).

use psx_formats::vab::{self, Bank};
use rebof3_emi::Entry;

use crate::registry::{Description, Format, RoundTrip, Selector, State};

pub mod vag;

/// The header as a format-registry row.
pub static HEADER: Description = Description {
    format: Format::VabHeader,
    name: "VAB header (type 6)",
    state: State::Codec,
    spec_state: "documented",
    codec_state: "decode+encode",
    evidence: "wiki: formats/audio#bank-layout; paired by load argument",
    extension: "vh",
    selector: Selector::Type(vab::HEADER_TYPE),
    round_trip: RoundTrip::Bank,
    derive: None,
    kind: "audio-bank",
};

/// The body as a format-registry row.
pub static BODY: Description = Description {
    format: Format::VabBody,
    name: "VAB body (type 7)",
    state: State::Codec,
    spec_state: "documented",
    codec_state: "decode+encode",
    evidence: "wiki: formats/audio#bank-layout; carried by its header",
    extension: "vb",
    selector: Selector::Type(vab::BODY_TYPE),
    round_trip: RoundTrip::Bank,
    derive: None,
    kind: "audio-bank",
};

/// Parses a header with its body and encodes the header again; `Err` names what differs.
pub fn round_trip(header: &[u8], body: &[u8]) -> Result<(), String> {
    let bank = Bank::parse(header, body).map_err(|error| error.to_string())?;
    if bank.encode_header() != header {
        return Err("the encoded header is not the header".into());
    }
    if bank.body_len() != body.len() {
        return Err("the parsed body is not the body entry's length".into());
    }
    Ok(())
}

/// Whether `entry` is the header of a bank.
pub fn is_header(entry: &Entry) -> bool {
    entry.file_type == vab::HEADER_TYPE
}
