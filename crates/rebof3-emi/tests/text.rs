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

#[test]
fn a_code_with_an_operand_keeps_its_operand() {
    let bytes = [0x05, 0x07, b'A', 0x00];
    assert_eq!(codec::decode(Encoding::Message, &bytes), "{COLOR:07}A{END}");
    assert_eq!(codec::encode(Encoding::Message, &codec::decode(Encoding::Message, &bytes)).unwrap(), bytes);
    let out = support::out_dir("a_code_with_an_operand_keeps_its_operand");
    support::write(&out, "decoded.txt", codec::decode(Encoding::Message, &bytes));
}
