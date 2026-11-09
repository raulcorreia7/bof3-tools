//! The scene and step table (`0x800E3800`): the actor definitions of an area
//! (`wiki: formats/area`).

use rebof3_emi::Entry;
use rebof3_emi::area::scene::SceneTable;

use crate::identity::difference;
use crate::registry::{Description, EMI_EXTENSION, Format, RoundTrip, Selector, State};

/// The load address of the table.
const LOAD_ADDRESS: u32 = 0x800e_3800;

/// The table as a format-registry row.
pub static ACTOR_DEFS: Description = Description {
    format: Format::AreaActorDefsTable,
    name: "area actor defs table (0x800E3800)",
    state: State::Codec,
    spec_state: "documented",
    codec_state: "decode+encode",
    evidence: "wiki: formats/area (`0x800E3800`)",
    extension: EMI_EXTENSION,
    selector: Selector::Load(&[LOAD_ADDRESS]),
    round_trip: RoundTrip::Payload(round_trip),
};

/// Decodes the table and encodes it again; `Err` names the first difference.
fn round_trip(_entry: &Entry, payload: &[u8]) -> Result<(), String> {
    let table = SceneTable::decode(payload).map_err(|error| error.to_string())?;
    difference(&table.encode(), payload)
}
