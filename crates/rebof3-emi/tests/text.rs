//! The name and ASCII codecs: every byte survives a decode and an encode.

mod support;

use rebof3_emi::text::codec::{self, Encoding};

#[test]
fn every_byte_string_survives_a_decode_and_an_encode() {
    for byte in 0..=u8::MAX {
        let bytes = [byte];
        let text = codec::decode(Encoding::Name, &bytes);
        assert_eq!(codec::encode(Encoding::Name, &text).unwrap(), bytes, "byte {byte:#04x}");
    }
}
