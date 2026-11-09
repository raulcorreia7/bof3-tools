//! The identity check every codec is held to: encoding what was decoded gives the stored bytes.

use rebof3_emi::verify::first_difference;

/// `Err` with the first difference when a codec's output is not its input.
pub fn difference(encoded: &[u8], payload: &[u8]) -> Result<(), String> {
    match first_difference(encoded, payload) {
        None => Ok(()),
        Some(offset) => Err(format!(
            "the encoded payload is not the payload ({} bytes against {}, first difference at \
             0x{offset:X})",
            encoded.len(),
            payload.len()
        )),
    }
}
