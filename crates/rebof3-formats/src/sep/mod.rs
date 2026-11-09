//! The SEP sequence package (entry type 10): the sequences of one sound bank
//! (`wiki: formats/audio`).

use psx_formats::sep::{self, Package};
use rebof3_emi::Entry;

use crate::identity::difference;
use crate::registry::{Description, Format, RoundTrip, Selector, State};

pub mod midi;

/// The package as a format-registry row.
pub static PACKAGE: Description = Description {
    format: Format::SepPackage,
    name: "SEP sequence package (type 10)",
    state: State::Codec,
    spec_state: "documented",
    codec_state: "decode+encode",
    evidence: "wiki: formats/audio; 13-byte headers tile 119/119",
    extension: "seq",
    selector: Selector::Type(sep::TYPE),
    round_trip: RoundTrip::Payload(round_trip),
    derive: None,
};

/// Parses the package and encodes it again; `Err` names the first difference.
fn round_trip(_entry: &Entry, payload: &[u8]) -> Result<(), String> {
    let package = Package::parse(payload).map_err(|error| error.to_string())?;
    difference(&package.encode(), payload)
}
