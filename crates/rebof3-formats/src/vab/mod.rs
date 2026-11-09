//! The VAB bank (entry types 6 and 7): the header with its programs and tones, and the body of
//! ADPCM samples that shares the header's load argument (`wiki: formats/audio#bank-layout`).

use psx_formats::vab::{self, Bank};
use rebof3_emi::Entry;

use crate::registry::{Description, Extra, Extras, Format, RoundTrip, Selector, Site, State, classify};

pub mod soundfont;
pub mod vag;

/// The sources behind the standard VAB file and the SoundFont.
const EVIDENCE: &str = "wiki: formats/audio#bank-layout (header and body share a load argument)";

/// The header with its body as the standard VAB file and as a SoundFont 2, when a body shares the
/// header's load argument.
fn open(site: &Site<'_>) -> Extras {
    let (entry, payload) = (site.entry(), site.payload());
    let body = site.entries.iter().enumerate().find_map(|(other, candidate)| {
        (candidate.ram_ptr == entry.ram_ptr && classify(candidate, site.payloads[other]) == Format::VabBody)
            .then_some(site.payloads[other])
    });
    let Some(body) = body else {
        return Extras {
            files: Vec::new(),
            skipped: Some("soundfont skipped: no body shares the header's load argument".to_string()),
        };
    };
    let extra =
        |role, extension, data| Extra { role, tag: String::new(), extension, evidence: EVIDENCE.to_string(), data };
    // The standard VAB file is the header followed by the body.
    let mut whole = payload.to_vec();
    whole.extend_from_slice(body);
    let mut extras = Extras { files: vec![extra("vab", "vab", whole)], skipped: None };
    match soundfont::export(payload, body, &format!("{}/{}", site.stem, site.slot)) {
        Ok(data) => extras.files.push(extra("sf2", "sf2", data)),
        Err(error) => extras.skipped = Some(format!("soundfont skipped: {error}")),
    }
    extras
}

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
    open: Some(open),
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
    open: None,
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
