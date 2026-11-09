//! A VAG sample as a WAVE file, and a WAVE file back to a VAG.

use psx_formats::adpcm;

use crate::error::{Error, Result};

/// The rate of a VAG at pitch `0x1000`, in hertz.
pub const VAG_RATE: u32 = 44_100;

/// The PCM of a VAG as a mono 16-bit WAVE file at [`VAG_RATE`].
pub fn pcm_wav(pcm: &[i16]) -> Result<Vec<u8>> {
    psx_formats::wav::pcm(pcm, VAG_RATE, 1).map_err(|error| Error::Audio(error.to_string()))
}

/// One VAG's stored bytes as a WAVE file at [`VAG_RATE`].
pub fn vag_wav(vag: &[u8]) -> Result<Vec<u8>> {
    pcm_wav(&psx_formats::adpcm::decode_stream(vag))
}

/// The VAG `index` of a VAB bank as a WAVE file.
pub fn vab_vag_wav(header: &[u8], body: &[u8], index: u32) -> Result<Vec<u8>> {
    let vab = psx_formats::vab::Bank::parse(header, body)?;
    pcm_wav(&vab.decode_vag(body, index)?)
}

/// Encodes a WAVE file as a VAG.
pub fn import(wav: &[u8]) -> Result<Vec<u8>> {
    Ok(adpcm::encode_stream(&read_pcm_wav(wav)?))
}

/// The samples of a mono 16-bit PCM WAVE file at [`VAG_RATE`].
fn read_pcm_wav(bytes: &[u8]) -> Result<Vec<i16>> {
    let bad = |reason: &str| Error::Import(format!("the WAVE file {reason}"));
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(bad("is not a RIFF WAVE file"));
    }
    let (mut format, mut data) = (None, None);
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let size = u32::from_le_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]]) as usize;
        let body = bytes.get(at + 8..at + 8 + size).ok_or_else(|| bad("has a chunk past its end"))?;
        match &bytes[at..at + 4] {
            b"fmt " => format = Some(body),
            b"data" => data = Some(body),
            _ => {}
        }
        at += 8 + size + (size & 1);
    }
    let format = format.filter(|format| format.len() >= 16).ok_or_else(|| bad("has no format chunk"))?;
    let data = data.ok_or_else(|| bad("has no data chunk"))?;
    let tag = u16::from_le_bytes([format[0], format[1]]);
    let channels = u16::from_le_bytes([format[2], format[3]]);
    let rate = u32::from_le_bytes([format[4], format[5], format[6], format[7]]);
    let bits = u16::from_le_bytes([format[14], format[15]]);
    if tag != 1 || channels != 1 || rate != VAG_RATE || bits != 16 {
        return Err(bad("must be mono 16-bit PCM at 44100 Hz"));
    }
    Ok(data.as_chunks::<2>().0.iter().map(|pair| i16::from_le_bytes(*pair)).collect())
}
