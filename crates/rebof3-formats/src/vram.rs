//! The VRAM texture upload (entry type 3): rectangles of 16-bit words the loader copies into the
//! frame buffer (`wiki: formats/graphics#chunk-layout`).

use psx_formats::vram::{self, Descriptor};
use rebof3_emi::Entry;

use crate::identity::difference;
use crate::registry::{Description, Format, RoundTrip, Selector, State};

/// The upload as a format-registry row.
pub static UPLOAD: Description = Description {
    format: Format::VramUpload,
    name: "VRAM texture upload (type 3)",
    state: State::Codec,
    spec_state: "documented",
    codec_state: "decode+encode",
    evidence: "wiki: formats/graphics#chunk-layout; descriptor from the load argument",
    extension: "img",
    selector: Selector::Type(vram::UPLOAD_TYPE),
    round_trip: RoundTrip::Payload(round_trip),
};

/// Decodes the upload and encodes it again; `Err` names the first difference.
fn round_trip(entry: &Entry, payload: &[u8]) -> Result<(), String> {
    let rectangles =
        vram::decode(payload, Descriptor::from_load_argument(entry.ram_ptr)).map_err(|error| error.to_string())?;
    difference(&vram::encode(&rectangles), payload)
}
