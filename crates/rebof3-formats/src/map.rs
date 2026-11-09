//! The map bundle (`0x80104000`): terrain, tiles, objects and layout of an area
//! (`wiki: formats/area`).

use rebof3_emi::Entry;
use rebof3_emi::area::map::MapBundle;

use crate::identity::difference;
use crate::registry::{Description, Format, RoundTrip, Selector, State};

/// The load address of the bundle.
const LOAD_ADDRESS: u32 = 0x8010_4000;

/// The bundle as a format-registry row.
pub static BUNDLE: Description = Description {
    format: Format::AreaMapBundle,
    name: "area map bundle (0x80104000)",
    state: State::Codec,
    spec_state: "documented",
    codec_state: "decode+encode",
    evidence: "wiki: formats/area (`0x80104000`)",
    extension: "map",
    selector: Selector::Load(&[LOAD_ADDRESS]),
    round_trip: RoundTrip::Payload(round_trip),
};

/// Decodes the bundle and encodes it again; `Err` names the first difference.
fn round_trip(_entry: &Entry, payload: &[u8]) -> Result<(), String> {
    let bundle = MapBundle::decode(payload).map_err(|error| error.to_string())?;
    difference(&bundle.encode(), payload)
}
