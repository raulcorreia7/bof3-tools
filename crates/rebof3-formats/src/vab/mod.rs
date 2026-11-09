//! The VAB bank (entry types 6 and 7): the header with its programs and tones, and the body of
//! ADPCM samples that shares the header's load argument (`wiki: formats/audio#bank-layout`).

use psx_formats::vab::{self, Bank};
use rebof3_emi::Entry;

use crate::error::Error;
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
    let whole = standard_file(payload, body);
    let mut extras = Extras { files: vec![extra("vab", "vab", whole)], skipped: None };
    match soundfont::export(payload, body, &format!("{}/{}", site.stem, site.slot)) {
        Ok(data) => extras.files.push(extra("sf2", "sf2", data)),
        Err(error) => extras.skipped = Some(format!("soundfont skipped: {error}")),
    }
    extras
}

/// The bank as one VAB file: the header followed by the body (Sony *File Formats*, chapter 4, "VAB").
///
/// The header's own length is fixed by its program count, so the file needs no separator. A studio's
/// tools read this file whole, or the header (`.vh`) and body (`.vb`) it splits into.
pub fn standard_file(header: &[u8], body: &[u8]) -> Vec<u8> {
    let mut whole = Vec::with_capacity(header.len() + body.len());
    whole.extend_from_slice(header);
    whole.extend_from_slice(body);
    whole
}

/// Bytes before the program tone tables: the 32-byte header and 128 program records of 16 bytes.
const TONE_TABLE: usize = 32 + 128 * 16;
/// Bytes of the tone table for each program: 16 tones of 32 bytes.
const TONE_BLOCK: usize = 16 * 32;
/// Bytes of the sample size table, 256 entries of 16 bits.
const SIZE_TABLE: usize = 256 * 2;
/// Offset of the 16-bit program count in the header.
const PROGRAM_COUNT: usize = 0x12;

/// A VAB file split into its header (`.vh`) and body (`.vb`) at the header's own length, and checked:
/// both halves must parse as a bank and the header must encode to the bytes it holds.
pub fn split_file(file: &[u8]) -> Result<(Vec<u8>, Vec<u8>), Error> {
    let count = file
        .get(PROGRAM_COUNT..PROGRAM_COUNT + 2)
        .map(|bytes| usize::from(u16::from_le_bytes([bytes[0], bytes[1]])))
        .filter(|_| file.starts_with(b"pBAV"))
        .ok_or_else(|| Error::Import("not a VAB file: it does not open with the pBAV header".into()))?;
    let size = TONE_TABLE + count * TONE_BLOCK + SIZE_TABLE;
    if file.len() < size {
        return Err(Error::Import(format!(
            "the VAB file is {} bytes; its header declares {count} programs and needs {size}",
            file.len()
        )));
    }
    let (header, body) = file.split_at(size);
    check_pair(header, body)?;
    Ok((header.to_vec(), body.to_vec()))
}

/// `Ok` when `header` and `body` are a bank: the header parses against the body, encodes back to its
/// own bytes, and the samples it sizes fit the body.
pub fn check_pair(header: &[u8], body: &[u8]) -> Result<(), Error> {
    let bank = Bank::parse(header, body)?;
    round_trip(header, body).map_err(Error::Import)?;
    let sized: usize = bank.vags.iter().map(|vag| vag.size as usize).sum();
    if sized > body.len() {
        return Err(Error::Import(format!("the header sizes {sized} bytes of samples; the body holds {}", body.len())));
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn bank() -> (Vec<u8>, Vec<u8>) {
        vab::encode_bank(3, &[], &[vec![0u8; 16], vec![1u8; 32]]).expect("a bank")
    }

    #[test]
    fn a_file_is_its_header_and_body() {
        let (header, body) = bank();
        let file = standard_file(&header, &body);
        assert_eq!(split_file(&file).unwrap(), (header, body));
    }

    #[test]
    fn a_pair_that_is_not_a_bank_is_refused() {
        let (header, body) = bank();
        assert!(split_file(b"MThd").is_err());
        assert!(split_file(&header[..100]).is_err());
        assert!(check_pair(&header, &body[..body.len() - 8]).is_err(), "the samples run past the body");
    }
}
