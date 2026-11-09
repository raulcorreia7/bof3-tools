//! The text kind's files: the stored payload, one JSON row per string and a plain-text view,
//! beside a run-wide index. The synthetic archive exercises the bank, a string table and the
//! payloads the scan drops; the retail disc is covered by the hands-on run in the task report.

mod support;

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use rebof3_assets::text;
use rebof3_emi::pack::{self, Payload};
use rebof3_emi::text::codec::{self, Encoding};
use rebof3_formats::message::bank;
use rebof3_formats::text::strings;

/// Every file under `directory`, relative to it and with forward slashes.
fn files_under(directory: &Path) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let Ok(entries) = fs::read_dir(directory) else { return found };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            for file in files_under(&path) {
                found.insert(format!("{}/{}", entry.file_name().to_string_lossy(), file));
            }
        } else {
            found.insert(entry.file_name().to_string_lossy().into_owned());
        }
    }
    found
}

fn messages() -> Vec<u8> {
    let texts = [
        codec::encode(Encoding::Message, "{BOX:05}Old man{NEWLINE}Is it true?{END}").unwrap(),
        codec::encode(Encoding::Message, "You won the battle{END}").unwrap(),
    ];
    let bank = bank::Bank { messages: texts.to_vec(), slots: vec![Some(0), Some(1), None] };
    bank::build(&bank).unwrap()
}

/// Four `8`-byte labels, the shape a menu table has.
fn labels() -> Vec<u8> {
    let mut payload = Vec::new();
    for name in ["Attack", "Magic", "Item", "Run"] {
        let mut field = name.as_bytes().to_vec();
        field.resize(8, 0);
        payload.extend(field);
    }
    payload
}

/// Bytes that decode to a few tags and letters, as sprite data in `PLCHAR` does.
fn sprite_noise() -> Vec<u8> {
    codec::encode(Encoding::Message, "{NUM:24}SJ{WAIT}B2M{SKIP12:46}QN{END}").unwrap()
}

fn archive(disc: &Path) -> PathBuf {
    let payloads = [
        Payload { bytes: &messages(), file_type: 0, ram_ptr: 0x8001_0000 },
        Payload { bytes: &labels(), file_type: 0, ram_ptr: 0x8001_a000 },
        Payload { bytes: &sprite_noise(), file_type: 1, ram_ptr: 0x8003_b800 },
    ];
    let bytes = pack::pack_payloads(1, &payloads).expect("the synthetic archive packs");
    let path = disc.join("BIN/ETC/TEST.EMI");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, bytes).unwrap();
    path
}

fn options(disc: &Path, out: &Path, raw: bool, modern: bool) -> text::Options {
    text::Options {
        disc: disc.to_path_buf(),
        out: out.to_path_buf(),
        image: None,
        raw,
        modern,
        write: true,
        list: false,
    }
}

#[test]
fn a_text_run_writes_the_payload_its_rows_and_the_index() {
    let scratch = support::out_dir("text_output-a_text_run_writes_the_payload_its_rows_and_the_index");
    let disc = scratch.join("disc");
    archive(&disc);
    let out = scratch.join("out");
    let report = text::extract_with(&options(&disc, &out, true, true)).expect("the run succeeds");
    assert!(report.failures.is_empty(), "{:?}", report.failures);

    let files = files_under(&out);
    for name in ["0.bin", "0.json", "0.txt", "1.bin", "1.json", "1.txt"] {
        assert!(files.contains(&format!("text/BIN/ETC/TEST/{name}")), "{name} is written: {files:?}");
    }
    assert!(!files.iter().any(|file| file.starts_with("text/BIN/ETC/TEST/2")), "the sprite payload writes nothing");
    assert!(files.contains("text/index.jsonl"));

    let bank_bytes = fs::read(out.join("text/BIN/ETC/TEST/0.bin")).unwrap();
    let bank_json = fs::read(out.join("text/BIN/ETC/TEST/0.json")).unwrap();
    assert_eq!(bank::import(&bank_json).unwrap(), bank_bytes);
    let document: serde_json::Value = serde_json::from_slice(&bank_json).unwrap();
    let row = &document["messages"][0];
    assert_eq!(row["source"], "BIN/ETC/TEST.EMI");
    assert_eq!(row["entry"], 0);
    assert_eq!(row["ram"], serde_json::json!(0x8001_0000u32 + row["offset"].as_u64().unwrap() as u32));
    assert_eq!(row["found"], "bank header");
    assert_eq!(row["kind"], "message");
    let start = row["offset"].as_u64().unwrap() as usize;
    let length = row["length"].as_u64().unwrap() as usize;
    assert_eq!(row["bytes"], serde_json::Value::String(hex(&bank_bytes[start..start + length])));

    let labels_bytes = fs::read(out.join("text/BIN/ETC/TEST/1.bin")).unwrap();
    let labels_json = fs::read(out.join("text/BIN/ETC/TEST/1.json")).unwrap();
    assert_eq!(strings::import(&labels_json, &labels_bytes).unwrap(), labels_bytes);
    let document: serde_json::Value = serde_json::from_slice(&labels_json).unwrap();
    assert_eq!(document["strings"][0]["kind"], "string-table");
    assert_eq!(document["strings"][0]["table"], "scan@0x8");
    assert_eq!(document["strings"][0]["id"], "scan@0x8[0]");
    assert_eq!(document["strings"][0]["found"], "scan");

    let plain = fs::read_to_string(out.join("text/BIN/ETC/TEST/1.txt")).unwrap();
    assert_eq!(plain.lines().next().unwrap(), "0x0\tscan@0x8[0]\tAttack{END}");
    let index = fs::read_to_string(out.join("text/index.jsonl")).unwrap();
    let rows: Vec<serde_json::Value> = index.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert!(rows.contains(&serde_json::json!({
        "source": "BIN/ETC/TEST.EMI#1+0x0",
        "kind": "string-table",
        "id": "scan@0x8[0]",
        "ram": "0x8001a000",
        "text": "Attack{END}"
    })));

    let artifacts = support::out_dir("text_output-bank_round_trip");
    support::write(&artifacts, "bank.bin", &bank_bytes);
    support::write(&artifacts, "bank.json", String::from_utf8(bank_json).unwrap());
    support::write(&artifacts, "bank.txt", fs::read_to_string(out.join("text/BIN/ETC/TEST/0.txt")).unwrap());
}

#[test]
fn raw_mode_writes_only_the_payloads() {
    let scratch = support::out_dir("text_output-raw_mode_writes_only_the_payloads");
    let disc = scratch.join("disc");
    archive(&disc);
    let out = scratch.join("out");
    text::extract_with(&options(&disc, &out, true, false)).unwrap();
    let files = files_under(&out);
    assert_eq!(files, BTreeSet::from(["text/BIN/ETC/TEST/0.bin".to_string(), "text/BIN/ETC/TEST/1.bin".to_string(),]));
}

#[test]
fn a_plan_lists_what_the_scan_drops() {
    let scratch = support::out_dir("text_output-a_plan_lists_what_the_scan_drops");
    let disc = scratch.join("disc");
    archive(&disc);
    let out = scratch.join("out");
    let mut plan = options(&disc, &out, true, true);
    plan.write = false;
    plan.list = true;
    let report = text::extract_with(&plan).unwrap();
    assert!(
        report
            .notes
            .iter()
            .any(|note| note == "text scan drop: BIN/ETC/TEST.EMI#2 character sprite blob, LZ77 (type 1)")
    );
    assert!(files_under(&out).is_empty(), "a plan writes nothing");
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
