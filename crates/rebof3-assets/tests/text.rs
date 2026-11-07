//! The game's text codec and the message banks. The retail checks run over every message bank on the owner's disc and
//! skip when no disc image is present (set `BOF3_DISC_IMAGE`). The synthetic checks cover the codes the disc does not
//! need in a bank of its own.

mod support;

use std::fs;
use std::path::Path;

use psx_cd::Image;
use rebof3_assets::disc::{self, Replacement};
use rebof3_assets::text::bank::{self, Bank};
use rebof3_assets::text::codec::{self, Encoding};
use rebof3_emi::Archive;
use rebof3_emi::classify::{Format, classify};

/// Every message bank on the disc: its archive's disc path, its entry index and its payload.
fn message_banks(cue: &Path) -> Vec<(String, usize, Vec<u8>)> {
    let mut image = Image::open(cue).expect("the disc opens");
    let mut banks = Vec::new();
    for file in image.entries().expect("the file table reads") {
        let is_emi = file.path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("EMI"));
        if file.is_directory || file.is_xa || !is_emi {
            continue;
        }
        let container = image.read_file(&file).expect("the archive reads");
        let Ok(archive) = Archive::parse(&container) else { continue };
        for (index, entry) in archive.entries().iter().enumerate() {
            let payload = archive.payload(&container, index).expect("the payload is in the archive");
            if classify(entry, payload) == Format::AreaMessageBank {
                banks.push((file.path.display().to_string(), index, payload.to_vec()));
            }
        }
    }
    banks
}

#[test]
fn every_message_bank_on_the_retail_disc_exports_and_imports_to_its_own_bytes() {
    let Some(cue) = support::retail_image() else {
        eprintln!("skipping message banks: no disc image (set BOF3_DISC_IMAGE)");
        return;
    };
    let banks = message_banks(&cue);
    assert!(!banks.is_empty(), "the disc has message banks");
    let mut messages = 0;
    for (path, index, payload) in &banks {
        let json = bank::export(payload).unwrap_or_else(|error| panic!("{path}#{index}: {error}"));
        let rebuilt = bank::import(&json).unwrap_or_else(|error| panic!("{path}#{index}: {error}"));
        assert!(rebuilt == *payload, "{path}#{index} differs after its export and import");
        for message in &bank::parse(payload).expect("the bank reads").messages {
            let text = codec::decode(Encoding::Message, message);
            let bytes = codec::encode(Encoding::Message, &text).expect("the text encodes");
            assert!(bytes == *message, "{path}#{index}: {text:?} does not encode to its bytes");
            messages += 1;
        }
    }
    let distinct: std::collections::BTreeSet<&[u8]> = banks.iter().map(|(_, _, payload)| payload.as_slice()).collect();
    eprintln!(
        "{} message entries ({} distinct payloads) and {messages} messages: each round trips to its own bytes",
        banks.len(),
        distinct.len()
    );
}

#[test]
fn the_retail_banks_rebuild_their_archives_and_patch_the_disc_to_its_own_bytes() {
    let Some(cue) = support::retail_image() else {
        eprintln!("skipping the message chain: no disc image (set BOF3_DISC_IMAGE)");
        return;
    };
    let mut image = Image::open(&cue).expect("the disc opens");
    let mut replacements = Vec::new();
    for file in image.entries().expect("the file table reads") {
        let is_emi = file.path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("EMI"));
        if file.is_directory || file.is_xa || !is_emi {
            continue;
        }
        let container = image.read_file(&file).expect("the archive reads");
        let Ok(archive) = Archive::parse(&container) else { continue };
        let mut replace = Vec::new();
        for (index, entry) in archive.entries().iter().enumerate() {
            let payload = archive.payload(&container, index).expect("the payload is in the archive");
            if classify(entry, payload) == Format::AreaMessageBank {
                let json = bank::export(payload).expect("the bank exports");
                replace.push((index, bank::import(&json).expect("the bank imports")));
            }
        }
        if replace.is_empty() {
            continue;
        }
        let replace: Vec<(usize, &[u8])> = replace.iter().map(|(index, bytes)| (*index, bytes.as_slice())).collect();
        let rebuilt = rebof3_assets::import::emi(&container, &replace).expect("the archive rebuilds");
        assert!(rebuilt == container, "{} differs after its banks are rebuilt", file.path.display());
        replacements.push(Replacement { path: file.path.clone(), bytes: rebuilt });
    }
    assert!(!replacements.is_empty(), "the disc has archives with message banks");

    let dir = support::out_dir("text-the_retail_banks_rebuild_their_archives_and_patch_the_disc_to_its_own_bytes");
    let output = dir.join("patched");
    let summary = disc::patch(&cue, &output, &replacements).expect("the disc patches");
    assert_eq!(summary.files, replacements.len());

    let patched_cue = output.join(cue.file_name().expect("the cue sheet has a name"));
    let mut patched = Image::open(&patched_cue).expect("the patched disc opens");
    let entries = patched.entries().expect("the patched file table reads");
    for replacement in &replacements {
        let entry = entries.iter().find(|entry| entry.path == replacement.path).expect("the archive is on the disc");
        assert!(patched.read_file(entry).expect("the archive reads back") == replacement.bytes);
    }
    eprintln!("{} archives rebuilt from their banks read back unchanged from the patched disc", replacements.len());
    fs::write(
        dir.join("patched.txt"),
        format!(
            "{} archives rebuilt from their message banks; the patched disc reads back unchanged.\n\
             The patched disc is a full copy and is not kept as an artifact.\n",
            replacements.len()
        ),
    )
    .expect("the patch summary");
    fs::remove_dir_all(&output).expect("the patched copy is removed");
}

#[test]
fn the_codec_keeps_every_code_and_writes_it_back() {
    // A letter, a space, a colour with its operand, the two fullwidth slots, a backspace, and a code with no operand
    // at the end of the string, which is kept as a raw byte.
    let raw = [0x41, 0xFF, 0x05, 0x02, 0x8D, 0x8F, 0x90, 0x04];
    let text = codec::decode(Encoding::Message, &raw);
    assert_eq!(text, "A {COLOR:02}\u{FF0C}\u{FF1A}{BACKSPACE}{RAW:04}");
    assert!(codec::encode(Encoding::Message, &text).expect("the text encodes") == raw);
}

#[test]
fn the_name_and_the_executable_strings_have_their_own_codes() {
    assert_eq!(codec::decode(Encoding::Name, &[0x01, 0x00]), "{RAW:01}{END}");
    assert_eq!(codec::decode(Encoding::Ascii, b"VSync: timeout\n\0"), "VSync: timeout{NEWLINE}{END}");
    assert_eq!(codec::decode(Encoding::Ascii, b"{"), "{RAW:7b}");
    assert!(codec::encode(Encoding::Ascii, "{RAW:7b}").expect("the raw brace encodes") == b"{");
    assert!(codec::encode(Encoding::Ascii, "{").expect("a bare brace is the byte") == b"{");
}

#[test]
fn text_that_names_no_code_or_a_wide_operand_is_refused() {
    for text in ["{NOPE}", "{COLOR:1ff}", "{RAW:100}", "{", "{COLOR}", "caf\u{E9}"] {
        assert!(codec::encode(Encoding::Message, text).is_err(), "{text:?} encodes");
    }
}

/// A bank of three messages and five slots: two slots share the first message, one slot names the end.
fn synthetic_bank() -> Vec<u8> {
    let messages = [
        codec::encode(Encoding::Message, "{BOX:05}Old man{NEWLINE}Is it true?{END}").expect("encodes"),
        codec::encode(Encoding::Message, "You won the battle{END}").expect("encodes"),
        codec::encode(Encoding::Message, "Choose{CHOICE:14}{END}").expect("encodes"),
    ];
    let bank = Bank { messages: messages.to_vec(), slots: vec![Some(0), Some(1), Some(0), Some(2), None] };
    bank::build(&bank).expect("the bank lays out")
}

#[test]
fn a_synthetic_bank_reads_back_its_layout_and_round_trips() {
    let payload = synthetic_bank();
    let parsed = bank::parse(&payload).expect("the bank reads");
    assert_eq!(parsed.slots, vec![Some(0), Some(1), Some(0), Some(2), None]);
    assert_eq!(parsed.messages.len(), 3);
    assert_eq!(bank::build(&parsed).expect("it lays out again"), payload);

    let json = bank::export(&payload).expect("the bank exports");
    assert!(bank::import(&json).expect("the bank imports") == payload);
}

#[test]
fn an_edited_message_takes_its_new_length_and_keeps_the_slots() {
    let payload = synthetic_bank();
    let json = String::from_utf8(bank::export(&payload).expect("the bank exports")).expect("JSON is UTF-8");
    let edited = json.replace("You won the battle{END}", "You won the battle, and {VAR:00} zenny!{END}");
    assert_ne!(edited, json, "the edit changes the document");

    let rebuilt = bank::import(edited.as_bytes()).expect("the edited bank imports");
    let parsed = bank::parse(&rebuilt).expect("the edited bank reads");
    assert_eq!(parsed.slots, vec![Some(0), Some(1), Some(0), Some(2), None]);
    assert_eq!(codec::decode(Encoding::Message, &parsed.messages[1]), "You won the battle, and {VAR:00} zenny!{END}");
    assert_eq!(codec::decode(Encoding::Message, &parsed.messages[2]), "Choose{CHOICE:14}{END}");
}

#[test]
fn a_bank_whose_layout_the_rule_does_not_describe_is_refused() {
    // The table names offset 2 for slot 0, which is inside the table itself.
    let inside_table = [0x04, 0x00, 0x02, 0x00, 0x41, 0x00];
    assert!(bank::parse(&inside_table).is_err());
    // The table names an offset past the end of the bank (6 bytes), where 6 itself is the end marker.
    let past_end = [0x04, 0x00, 0x07, 0x00, 0x41, 0x00];
    assert!(bank::parse(&past_end).is_err());
}
