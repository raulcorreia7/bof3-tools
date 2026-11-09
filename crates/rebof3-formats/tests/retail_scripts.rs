//! Every retail script bank decodes to its ops and back byte for byte, its text form imports to the
//! stored bytes, and every op stays where the step functions would step: a jump lands on a step of
//! its stream and a shown frame's reads stay in the bank.
//!
//! Point `BOF3_DISC_DIR` at an extracted disc; without it the test skips.

mod support;

use rebof3_emi::Archive;
use rebof3_formats::Format;
use rebof3_formats::classify;
use rebof3_formats::script::ScriptBank;
use rebof3_formats::script::program::{Coverage, Op, Program};
use rebof3_formats::script::text;

/// The pinned reach of the decode, per bank format: banks, records, streams, ops and shown frames
/// past their frame table.
struct Pinned {
    format: Format,
    banks: usize,
    records: usize,
    streams: usize,
    ops: usize,
    frames_past_table: usize,
    draws_past_bank: usize,
}

const PINNED: [Pinned; 3] = [
    Pinned {
        format: Format::AreaScriptBank,
        banks: 200,
        records: 13_487,
        streams: 13_487,
        ops: 65_816,
        frames_past_table: 3,
        draws_past_bank: 0,
    },
    Pinned {
        format: Format::BattleScriptBank,
        banks: 42,
        records: 1_554,
        streams: 1_554,
        ops: 8_316,
        frames_past_table: 84,
        draws_past_bank: 84,
    },
    Pinned {
        format: Format::MagicTable,
        banks: 37,
        records: 95,
        streams: 72,
        ops: 820,
        frames_past_table: 1,
        draws_past_bank: 0,
    },
];

/// Where each record of `bank` starts in the payload, per section.
fn record_starts(bank: &ScriptBank) -> Vec<Vec<usize>> {
    let mut base = bank.dir_bytes as usize;
    let mut starts = Vec::new();
    for section in &bank.sections {
        let mut at = base + section.table_bytes as usize;
        let mut section_starts = Vec::new();
        for record in &section.records {
            section_starts.push(at);
            at += record.len();
        }
        base = at;
        starts.push(section_starts);
    }
    starts
}

/// Bytes of one frame-table word.
const FRAME_WORD: usize = 2;

/// The faults of one bank: a rebuild that differs, a jump off the stream, a frame word off the bank.
/// A shown frame whose cells reach past the bank (a frame past the table, whose word the game reads
/// from the frames after it) is counted in `draws_past_bank`, not a fault: the retail data holds it.
fn check(name: &str, payload: &[u8], failures: &mut Vec<String>, draws_past_bank: &mut usize) -> Option<Program> {
    let program = match Program::decode(payload) {
        Ok(program) => program,
        Err(error) => {
            failures.push(format!("{name}: {error}"));
            return None;
        }
    };
    if program.encode().ok().as_deref() != Some(payload) {
        failures.push(format!("{name}: the ops do not encode to the stored bytes"));
    }
    match text::export(payload).and_then(|listing| text::import(&listing, payload)) {
        Ok(bytes) if bytes == payload => {}
        Ok(_) => failures.push(format!("{name}: the text imports to other bytes")),
        Err(error) => failures.push(format!("{name}: text: {error}")),
    }

    let bank = ScriptBank::decode(payload).expect("the program decoded the bank");
    for ((section_index, section), starts) in program.sections.iter().enumerate().zip(record_starts(&bank)) {
        for ((record_index, record), start) in section.records.iter().enumerate().zip(starts) {
            let Some(stream) = record.stream() else { continue };
            let steps = stream.animation.steps.len();
            for (step, op) in stream.ops().enumerate() {
                let place = format!("{name} section {section_index} record {record_index} step {step}");
                match op {
                    Op::Jump { target, .. } if usize::from(target) >= steps => {
                        failures.push(format!("{place}: the jump lands past the stream"));
                    }
                    Op::Show { frame, .. } => {
                        if start + stream.frame_word(frame) + FRAME_WORD > payload.len() {
                            failures.push(format!("{place}: the frame word of frame {frame} is past the bank"));
                        } else if stream.frame_reach(frame).is_none_or(|reach| start + reach.end > payload.len()) {
                            *draws_past_bank += 1;
                        }
                    }
                    Op::Jump { .. } => {}
                }
            }
        }
    }
    Some(program)
}

#[test]
fn every_retail_script_bank_decodes_to_ops_and_back() {
    let Some(root) = support::retail_root() else {
        return;
    };

    let mut coverage: Vec<Coverage> = PINNED.iter().map(|_| Coverage::default()).collect();
    let mut draws_past_bank = [0; PINNED.len()];
    let mut failures = Vec::new();
    for path in rebof3_emi::archives_under(&root) {
        let bytes = std::fs::read(&path).expect("the archive reads");
        let archive = Archive::parse(&bytes).expect("the archive parses");
        for (index, entry) in archive.entries().iter().enumerate() {
            let payload = archive.payload(&bytes, index).expect("the payload reads");
            let format = classify(entry, payload);
            let Some(row) = PINNED.iter().position(|pinned| pinned.format == format) else { continue };
            let name = format!("{}#{index}", path.display());
            if let Some(program) = check(&name, payload, &mut failures, &mut draws_past_bank[row]) {
                coverage[row].add(&program);
            }
        }
    }

    for ((pinned, found), draws) in PINNED.iter().zip(&coverage).zip(draws_past_bank) {
        eprintln!(
            "{}: {} banks, {} records, {} streams, {} ops, {:.1}% with a known opcode, unknown {:?}, \
             {} frames past the table, {draws} drawn past the bank",
            pinned.format.name(),
            found.banks,
            found.records,
            found.streams,
            found.ops,
            found.known_percent(),
            found.unknown,
            found.frames_past_table
        );
    }
    assert!(failures.is_empty(), "{} failures:\n{}", failures.len(), failures[..failures.len().min(20)].join("\n"));
    for ((pinned, found), draws) in PINNED.iter().zip(&coverage).zip(draws_past_bank) {
        let name = pinned.format.name();
        assert_eq!(found.banks, pinned.banks, "{name}: banks");
        assert_eq!(found.records, pinned.records, "{name}: records");
        assert_eq!(found.streams, pinned.streams, "{name}: streams");
        assert_eq!(found.ops, pinned.ops, "{name}: ops");
        assert_eq!(found.known, found.ops, "{name}: every op has a known opcode");
        assert!(found.unknown.is_empty(), "{name}: unknown opcodes {:?}", found.unknown);
        assert_eq!(found.frames_past_table, pinned.frames_past_table, "{name}: frames past the table");
        assert_eq!(draws, pinned.draws_past_bank, "{name}: frames drawn past the bank");
    }
}
