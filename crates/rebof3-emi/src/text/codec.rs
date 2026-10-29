//! The game's text encodings: each byte is a character, a control code or a `{TAG}` marker.
//!
//! `decode` never drops a byte. A code without a known meaning is `{RAW:xx}`, and `encode` writes that tag back as
//! its byte, so `encode(decode(bytes)) == bytes` for every byte string. The meanings and the evidence for them are in
//! `wiki: game/text`.

use std::fmt::Write as _;

use crate::{Error, Result};

/// One text encoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Encoding {
    /// Dialogue, battle and menu text in the game's own codes, with their operands.
    Message,
    /// A name field: the same bytes as a message, but its only single-byte control is the terminator.
    Name,
    /// The executable's C string literals: printable ASCII, with a few whitespace and terminator codes.
    Ascii,
}

/// Punctuation that draws as one byte. The high slots `0x8D` and `0x8F` are spelled with the fullwidth characters,
/// so each slot keeps its own spelling.
const PUNCTUATION: &[(u8, char)] = &[
    (0x3A, ':'),
    (0x3B, ';'),
    (0x3C, ','),
    (0x3D, '-'),
    (0x3E, '.'),
    (0x5C, '?'),
    (0x5D, '!'),
    (0x8B, '+'),
    (0x8D, '\u{FF0C}'),
    (0x8E, '\''),
    (0x8F, '\u{FF1A}'),
    (0xFF, ' '),
];

/// Codes that take no operand, with their tags.
const MESSAGE_SINGLE: &[(u8, &str)] = &[
    (0x00, "END"),
    (0x01, "NEWLINE"),
    (0x02, "PAGE"),
    (0x03, "GOTO"),
    (0x06, "NOCOLOR"),
    (0x09, "NOP09"),
    (0x0B, "GAP"),
    (0x0D, "NOP0D"),
    (0x0E, "NOP0E"),
    (0x0F, "STYLE0F"),
    (0x10, "STYLE10"),
    (0x11, "WAIT"),
    (0x90, "BACKSPACE"),
];
const NAME_SINGLE: &[(u8, &str)] = &[(0x00, "END")];
const ASCII_SINGLE: &[(u8, &str)] = &[(0x00, "END"), (0x09, "TAB"), (0x0A, "NEWLINE"), (0x0D, "CR")];

/// Codes followed by one operand byte, with their tags. The operand is written as two hex digits.
const OPERAND: &[(u8, &str)] = &[
    (0x04, "INSERT"),
    (0x05, "COLOR"),
    (0x07, "VAR"),
    (0x08, "LINK"),
    (0x0A, "SE"),
    (0x0C, "BOX"),
    (0x12, "SKIP12"),
    (0x13, "SKIP13"),
    (0x14, "CHOICE"),
    (0x15, "NUM"),
    (0x16, "BOXEND"),
];

impl Encoding {
    pub(crate) fn singles(self) -> &'static [(u8, &'static str)] {
        match self {
            Self::Message => MESSAGE_SINGLE,
            Self::Name => NAME_SINGLE,
            Self::Ascii => ASCII_SINGLE,
        }
    }

    pub(crate) fn operands(self) -> &'static [(u8, &'static str)] {
        match self {
            Self::Message | Self::Name => OPERAND,
            Self::Ascii => &[],
        }
    }

    /// The character a byte draws as, or `None` when the byte is not a character of this encoding.
    pub(crate) fn glyph(self, code: u8) -> Option<char> {
        match self {
            Self::Ascii => (0x20..=0x7E).contains(&code).then_some(char::from(code)),
            Self::Message | Self::Name => match code {
                0x30..=0x39 | 0x41..=0x5A | 0x61..=0x7A => Some(char::from(code)),
                _ => PUNCTUATION.iter().find(|(byte, _)| *byte == code).map(|(_, glyph)| *glyph),
            },
        }
    }

    /// The byte a character is written as, or `None` when the encoding has no such character.
    fn code_of(self, glyph: char) -> Option<u8> {
        match self {
            Self::Ascii => (' '..='~').contains(&glyph).then_some(glyph as u8),
            Self::Message | Self::Name => {
                if glyph.is_ascii_alphanumeric() {
                    Some(glyph as u8)
                } else {
                    PUNCTUATION.iter().find(|(_, mark)| *mark == glyph).map(|(byte, _)| *byte)
                }
            }
        }
    }
}

fn tag_for(table: &[(u8, &'static str)], code: u8) -> Option<&'static str> {
    table.iter().find(|(byte, _)| *byte == code).map(|(_, tag)| *tag)
}

fn code_for(table: &[(u8, &'static str)], tag: &str) -> Option<u8> {
    table.iter().find(|(_, name)| *name == tag).map(|(byte, _)| *byte)
}

/// Renders `raw` as text with `{TAG}` markers. Never drops a byte.
pub fn decode(encoding: Encoding, raw: &[u8]) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut at = 0;
    while at < raw.len() {
        let code = raw[at];
        if let Some(tag) = tag_for(encoding.singles(), code) {
            let _ = write!(out, "{{{tag}}}");
            at += 1;
            continue;
        }
        if let (Some(tag), Some(operand)) = (tag_for(encoding.operands(), code), raw.get(at + 1)) {
            let _ = write!(out, "{{{tag}:{operand:02x}}}");
            at += 2;
            continue;
        }
        match encoding.glyph(code) {
            // A brace opens a tag in the text, so the ASCII brace is written as its byte.
            Some(glyph) if glyph != '{' => out.push(glyph),
            _ => {
                let _ = write!(out, "{{RAW:{code:02x}}}");
            }
        }
        at += 1;
    }
    out
}

/// One `{BODY}` or `{BODY:hex}` marker read from the text.
struct Tag {
    body: String,
    operand: Option<String>,
    end: usize,
}

/// The marker that opens at `at`, if the characters there form one. `{` `BODY` is an uppercase letter followed by
/// uppercase letters or digits; an operand is `:` and one or more hex digits.
fn read_tag(chars: &[char], at: usize) -> Option<Tag> {
    let mut end = at + 1;
    if !chars.get(end)?.is_ascii_uppercase() {
        return None;
    }
    let body_start = end;
    while chars.get(end).is_some_and(|c| c.is_ascii_uppercase() || c.is_ascii_digit()) {
        end += 1;
    }
    let body: String = chars[body_start..end].iter().collect();
    let mut operand = None;
    if chars.get(end) == Some(&':') {
        let operand_start = end + 1;
        end = operand_start;
        while chars.get(end).is_some_and(char::is_ascii_hexdigit) {
            end += 1;
        }
        if end == operand_start {
            return None;
        }
        operand = Some(chars[operand_start..end].iter().collect());
    }
    (chars.get(end) == Some(&'}')).then_some(Tag { body, operand, end: end + 1 })
}

/// Whether a marker names something this encoding writes back: `RAW` with a byte, a single-byte code, or a code with
/// an operand.
fn is_known(encoding: Encoding, tag: &Tag) -> bool {
    match (tag.body.as_str(), &tag.operand) {
        ("RAW", operand) => operand.is_some(),
        (body, None) => tag_is_listed(encoding.singles(), body),
        (body, Some(_)) => tag_is_listed(encoding.operands(), body),
    }
}

fn tag_is_listed(table: &[(u8, &str)], body: &str) -> bool {
    table.iter().any(|(_, name)| *name == body)
}

/// Writes `text` back as bytes: the inverse of [`decode`] for any string it produced.
pub fn encode(encoding: Encoding, text: &str) -> Result<Vec<u8>> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::with_capacity(chars.len());
    let mut at = 0;
    while at < chars.len() {
        if chars[at] == '{'
            && let Some(tag) = read_tag(&chars, at).filter(|tag| is_known(encoding, tag))
        {
            push_tag(encoding, &tag, &mut out)?;
            at = tag.end;
            continue;
        }
        let glyph = chars[at];
        let code = encoding
            .code_of(glyph)
            .ok_or_else(|| Error::Text(format!("character {glyph:?} at {at} is not in the {encoding:?} encoding")))?;
        out.push(code);
        at += 1;
    }
    Ok(out)
}

/// Writes one marker that [`is_known`] accepted.
fn push_tag(encoding: Encoding, tag: &Tag, out: &mut Vec<u8>) -> Result<()> {
    let body = tag.body.as_str();
    match &tag.operand {
        Some(hex) if body == "RAW" => out.push(byte_of(hex, body)?),
        None => {
            let code =
                code_for(encoding.singles(), body).ok_or_else(|| Error::Text(format!("unknown tag {{{body}}}")))?;
            out.push(code);
        }
        Some(hex) => {
            let code = code_for(encoding.operands(), body)
                .ok_or_else(|| Error::Text(format!("unknown tag {{{body}:{hex}}}")))?;
            out.extend_from_slice(&[code, byte_of(hex, body)?]);
        }
    }
    Ok(())
}

fn byte_of(hex: &str, body: &str) -> Result<u8> {
    u8::from_str_radix(hex, 16).map_err(|_| Error::Text(format!("{{{body}:{hex}}} does not fit one byte")))
}
