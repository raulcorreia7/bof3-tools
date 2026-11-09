//! Every retail script table decodes to its ops and back byte for byte, its text form imports to the stored

mod support;

use rebof3_emi::Archive;
use rebof3_formats::Format;
use rebof3_formats::classify;
use rebof3_formats::script::ScriptTable;
use rebof3_formats::script::program::{Coverage, Op, Program};
use rebof3_formats::script::text;

/// The pinned reach of the decode, per table format.
struct Pinned {
    format: Format,
    tables: usize,
    records: usize,
    streams: usize,
    ops: usize,
    frames_past_table: usize,
    draws_past_table: usize,
}

const PINNED: [Pinned; 3] = [
    Pinned {
        format: Format::AreaScriptTable,
        tables: 200,
        records: 13_487,
        streams: 13_487,
        ops: 65_816,
        frames_past_table: 3,
        draws_past_table: 0,
    },
    Pinned {
        format: Format::BattleScriptTable,
        tables: 42,
        records: 1_554,
        streams: 1_554,
        ops: 8_316,
        frames_past_table: 84,
        draws_past_table: 84,
    },
    Pinned {
        format: Format::MagicScriptTable,
        tables: 37,
        records: 95,
        streams: 72,
        ops: 820,
        frames_past_table: 1,
        draws_past_table: 0,
    },
];

/// Where each record of `table` starts in the payload, per section.
fn record_starts(table: &ScriptTable) -> Vec<Vec<usize>> {
    let mut base = table.dir_bytes as usize;
    let mut starts = Vec::new();
    for section in &table.sections {
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

/// The faults of one table.
fn check(name: &str, payload: &[u8], failures: &mut Vec<String>, draws_past_table: &mut usize) -> Option<Program> {
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

    let table = ScriptTable::decode(payload).expect("the program decoded the table");
    for ((section_index, section), starts) in program.sections.iter().enumerate().zip(record_starts(&table)) {
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
                            failures.push(format!("{place}: the frame word of frame {frame} is past the table"));
                        } else if stream.frame_reach(frame).is_none_or(|reach| start + reach.end > payload.len()) {
                            *draws_past_table += 1;
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
fn every_retail_script_table_decodes_to_ops_and_back() {
    let Some(root) = support::retail_root() else {
        return;
    };

    let mut coverage: Vec<Coverage> = PINNED.iter().map(|_| Coverage::default()).collect();
    let mut draws_past_table = [0; PINNED.len()];
    let mut failures = Vec::new();
    for path in rebof3_emi::archives_under(&root) {
        let bytes = std::fs::read(&path).expect("the archive reads");
        let archive = Archive::parse(&bytes).expect("the archive parses");
        for (index, entry) in archive.entries().iter().enumerate() {
            let payload = archive.payload(&bytes, index).expect("the payload reads");
            let format = classify(entry, payload);
            let Some(row) = PINNED.iter().position(|pinned| pinned.format == format) else { continue };
            let name = format!("{}#{index}", path.display());
            if let Some(program) = check(&name, payload, &mut failures, &mut draws_past_table[row]) {
                coverage[row].add(&program);
            }
        }
    }

    for ((pinned, found), draws) in PINNED.iter().zip(&coverage).zip(draws_past_table) {
        eprintln!(
            "{}: {} tables, {} records, {} streams, {} ops, {:.1}% with a known opcode, unknown {:?}, \
             {} frames past the table, {draws} drawn past the table",
            pinned.format.name(),
            found.tables,
            found.records,
            found.streams,
            found.ops,
            found.known_percent(),
            found.unknown,
            found.frames_past_table
        );
    }
    assert!(failures.is_empty(), "{} failures:\n{}", failures.len(), failures[..failures.len().min(20)].join("\n"));
    for ((pinned, found), draws) in PINNED.iter().zip(&coverage).zip(draws_past_table) {
        let name = pinned.format.name();
        assert_eq!(found.tables, pinned.tables, "{name}: tables");
        assert_eq!(found.records, pinned.records, "{name}: records");
        assert_eq!(found.streams, pinned.streams, "{name}: streams");
        assert_eq!(found.ops, pinned.ops, "{name}: ops");
        assert_eq!(found.known, found.ops, "{name}: every op has a known opcode");
        assert!(found.unknown.is_empty(), "{name}: unknown opcodes {:?}", found.unknown);
        assert_eq!(found.frames_past_table, pinned.frames_past_table, "{name}: frames past the table");
        assert_eq!(draws, pinned.draws_past_table, "{name}: frames drawn past the table");
    }
}
