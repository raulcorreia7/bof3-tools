//! A message bank as a PO catalogue for translators (GNU gettext, "PO Files").
//!
//! Each message is one entry: `msgctxt` is the stable id `message[<first slot>]`, `msgid` is the message's text with
//! its control codes as `{TAG}` markup (`rebof3_emi::text::codec`), and `msgstr` is the translation. An empty
//! `msgstr`, or one equal to the `msgid`, keeps the original bytes, so an untouched catalogue rebuilds the bank byte
//! for byte. A translated `msgstr` is encoded with the same markup, so its codes, its `{END}` and its padding are the
//! translator's to keep.

use rebof3_emi::text::codec::{self, Encoding};

use super::bank::{self, Bank};
use crate::error::{Error, Result};

const HEADER: &str = "msgid \"\"\nmsgstr \"\"\n\"Content-Type: text/plain; charset=UTF-8\\n\"\n\n";

/// The bank as a PO catalogue, one entry per message, with an empty `msgstr` for each.
pub fn export(payload: &[u8]) -> Result<Vec<u8>> {
    let parsed = bank::parse(payload)?;
    let mut out = String::from(HEADER);
    for (index, message) in parsed.messages.iter().enumerate() {
        out.push_str(&format!("#. {} bytes\n", message.len()));
        out.push_str(&format!("msgctxt \"{}\"\n", id(&parsed, index)));
        out.push_str(&format!("msgid \"{}\"\nmsgstr \"\"\n\n", escape(&codec::decode(Encoding::Message, message))));
    }
    Ok(out.into_bytes())
}

/// The bank bytes of `payload` with each translated message of `catalogue` put in place. Refuses an entry whose
/// `msgctxt` names no message, text that does not encode, and a bank past its limit.
pub fn import(catalogue: &[u8], payload: &[u8]) -> Result<Vec<u8>> {
    let text =
        std::str::from_utf8(catalogue).map_err(|error| Error::Import(format!("the PO file is not UTF-8: {error}")))?;
    let mut parsed = bank::parse(payload)?;
    for entry in entries(text)? {
        let Some(context) = entry.context else { continue };
        let index = (0..parsed.messages.len())
            .find(|&index| id(&parsed, index) == context)
            .ok_or_else(|| Error::Import(format!("the PO entry {context} names no message of the bank")))?;
        if entry.translation.is_empty() || entry.translation == entry.source {
            continue;
        }
        parsed.messages[index] = codec::encode(Encoding::Message, &entry.translation)
            .map_err(|error| Error::Import(format!("{context}: {error}")))?;
    }
    bank::build_within_limit(&parsed)
}

/// `message[<first slot>]`, the id `bank::export` gives too.
fn id(bank: &Bank, message: usize) -> String {
    let slot = bank.slots.iter().position(|slot| *slot == Some(message)).unwrap_or(message);
    format!("message[{slot}]")
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

struct Entry {
    context: Option<String>,
    source: String,
    translation: String,
}

/// Which string of an entry a continuation line extends.
#[derive(Clone, Copy)]
enum Field {
    Context,
    Source,
    Translation,
}

fn entries(text: &str) -> Result<Vec<Entry>> {
    let mut found = Vec::new();
    let mut current: Option<Entry> = None;
    let mut field = Field::Source;
    for (number, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            if line.is_empty() {
                found.extend(current.take());
            }
            continue;
        }
        let (keyword, rest) = match line.split_once(' ') {
            Some((keyword, rest)) if !keyword.starts_with('"') => (keyword, rest),
            _ => ("", line),
        };
        let quoted = rest
            .strip_prefix('"')
            .and_then(|rest| rest.strip_suffix('"'))
            .map(unescape)
            .ok_or_else(|| Error::Import(format!("PO line {}: expected a quoted string", number + 1)))?;
        let entry = current.get_or_insert(Entry { context: None, source: String::new(), translation: String::new() });
        match keyword {
            "msgctxt" => {
                field = Field::Context;
                entry.context = Some(String::new());
            }
            "msgid" => {
                field = Field::Source;
                entry.source.clear();
            }
            "msgstr" => {
                field = Field::Translation;
                entry.translation.clear();
            }
            "" => {}
            other => return Err(Error::Import(format!("PO line {}: unknown keyword {other}", number + 1))),
        }
        match field {
            Field::Context => entry.context.get_or_insert_with(String::new).push_str(&quoted),
            Field::Source => entry.source.push_str(&quoted),
            Field::Translation => entry.translation.push_str(&quoted),
        }
    }
    found.extend(current);
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<u8> {
        let mut bank = vec![0u8; 4];
        bank[0] = 4;
        bank[2] = 4;
        bank.extend(b"Hi");
        bank.push(0);
        bank
    }

    #[test]
    fn an_untouched_catalogue_rebuilds_the_bank() {
        let bank = sample();
        let po = export(&bank).unwrap();
        assert_eq!(import(&po, &bank).unwrap(), bank);
    }

    #[test]
    fn a_translation_replaces_its_message() {
        let bank = sample();
        let po =
            String::from_utf8(export(&bank).unwrap()).unwrap().replace("msgstr \"\"\n\n", "msgstr \"Yo{END}\"\n\n");
        let rebuilt = import(po.as_bytes(), &bank).unwrap();
        assert_eq!(&rebuilt[4..], b"Yo\0");
    }
}
