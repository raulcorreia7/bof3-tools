//! A script table read as the programs its records hold: one animation stream per record.
//!
//! `Actor_BeginScript` (`exe/slus_004_22@0x8014D6DC`) points work `+0x50` at a record of the table
//! `g_ScriptBank` (`exe/slus_004_22@0x801459F0`) names, and `Actor_SeekScript`
//! (`exe/slus_004_22@0x8014D910`) reads it as an [`Animation`]: two counts, the steps, the frame
//! table and the frames. No other reader takes a record, so a record is an animation stream and its
//! ops are the steps. The step functions read the first byte of a step to choose what it does; the
//! two choices are [`OPCODES`], and every byte value selects one of them.
//!
//! A record that does not read as a stream is kept as [`Record::Raw`] with the reason; in the retail
//! tables only empty records do. The bytes between a stream's end and the next record are its
//! padding: zeros that keep the next record two-byte aligned for the `u16` frame table reads.

use core::ops::RangeInclusive;
use std::collections::BTreeMap;

use crate::animation::{Animation, CELL_BYTES, Cell, JUMP, Step};
use crate::error::{Error, Result};

use super::{ScriptSection, ScriptTable};

/// The size of one directory word and of one table word, as [`ScriptTable`] reads them.
const DIRECTORY_WORD: usize = 4;
const TABLE_WORD: usize = 2;
/// Bytes of the two counts that open a stream, of one step and of one frame-table word.
const COUNTS_BYTES: usize = 2;
const STEP_BYTES: usize = 2;
const FRAME_WORD: usize = 2;
/// The most steps a stream holds: its step count is one byte (`Actor_SeekScript` stages it at `+0x49`).
pub const MAX_STEPS: usize = u8::MAX as usize;
/// The alignment an edit keeps for every record after the edited one: the padding grows or shrinks
/// so the record's stored length keeps its remainder by this.
const KEPT_ALIGNMENT: usize = 4;
/// The farthest a record may start from its section base: the table words are `u16`.
const MAX_RECORD_OFFSET: usize = u16::MAX as usize;

/// What a step's byte means to the handler that reads it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operand {
    /// Ticks the frame stays on screen: the byte goes to work `+0x4A` and is counted down once a
    /// tick before it is tested, so 0 lasts 256 ticks.
    Ticks,
    /// The frame to show: an index into the frame table, staged as the frame's offset in work `+0x5A`.
    Frame,
    /// The byte that makes the step a jump, any value from [`JUMP`] up.
    Code,
    /// The step to continue at: the position becomes twice the byte.
    Target,
}

/// One opcode: the first-byte values that select it, its length and its two bytes as the handler
/// reads them.
#[derive(Debug)]
pub struct Opcode {
    /// The name the listing and the text form use.
    pub mnemonic: &'static str,
    /// The first-byte values that select it.
    pub codes: RangeInclusive<u8>,
    /// Bytes the step position advances by.
    pub bytes: usize,
    /// What byte 0 and byte 1 mean.
    pub operands: [Operand; 2],
    /// The handler, as `function (image@address)`.
    pub handler: &'static str,
    /// What the handler does.
    pub does: &'static str,
}

/// The two opcodes of the step stream, read from the step functions.
pub static OPCODES: [Opcode; 2] = [
    // `Actor_StageAnimStep` (exe/slus_004_22@0x8014DB98): `unk_4A = script[pc]`, `selected = script[pc + 1]`,
    // `pc += 2`, `unk_5A = frame_table[selected]`. The loop and once step functions call it for every
    // step whose first byte is under 0x80.
    Opcode {
        mnemonic: "show",
        codes: 0x00..=JUMP - 1,
        bytes: STEP_BYTES,
        operands: [Operand::Ticks, Operand::Frame],
        handler: "Actor_StageAnimStep (exe/slus_004_22@0x8014DB98)",
        does: "stages the frame and the ticks it stays",
    },
    // `Actor_StepAnimLoop` (exe/slus_004_22@0x8014D978) and `Actor_StepAnimOnce` (exe/slus_004_22@0x8014DAEC): at
    // an expired delay, `if (script[pc] >= 0x80) pc = script[pc + 1] << 1`, then `Actor_StageAnimStep` stages
    // the step there. `Actor_StepAnimReverse` (exe/slus_004_22@0x8014DA24), which plays a stream backwards,
    // does not test the byte and shows the step as a frame for 128 or more ticks.
    Opcode {
        mnemonic: "jump",
        codes: JUMP..=0xFF,
        bytes: STEP_BYTES,
        operands: [Operand::Code, Operand::Target],
        handler: "Actor_StepAnimLoop (exe/slus_004_22@0x8014D978), Actor_StepAnimOnce (exe/slus_004_22@0x8014DAEC)",
        does: "continues at the target step",
    },
];

/// The opcode a step's first byte selects, if any handler reads it.
pub fn opcode_of(code: u8) -> Option<&'static Opcode> {
    OPCODES.iter().find(|opcode| opcode.codes.contains(&code))
}

/// A step read as its op.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Op {
    /// Shows `frame` for `ticks` (the stored byte; 0 lasts 256 ticks).
    Show { ticks: u8, frame: u8 },
    /// Continues at step `target`; `code` is the stored first byte, [`JUMP`] in every retail stream.
    Jump { code: u8, target: u8 },
}

impl Op {
    /// The op of a stored step.
    pub fn of(step: Step) -> Self {
        if step.delay >= JUMP { Self::Jump { code: step.delay, target: step.frame } } else { Self::Show { ticks: step.delay, frame: step.frame } }
    }

    /// The stored step.
    pub fn step(self) -> Step {
        match self {
            Self::Show { ticks, frame } => Step { delay: ticks, frame },
            Self::Jump { code, target } => Step { delay: code, frame: target },
        }
    }

    /// The opcode that runs it.
    pub fn opcode(self) -> &'static Opcode {
        match self {
            Self::Show { .. } => &OPCODES[0],
            Self::Jump { .. } => &OPCODES[1],
        }
    }
}

/// A record read as an animation stream, and the bytes after it up to the next record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Stream {
    pub animation: Animation,
    /// The bytes between the stream's end and the next record, as stored.
    pub padding: Vec<u8>,
}

impl Stream {
    /// The ops, in stream order.
    pub fn ops(&self) -> impl Iterator<Item = Op> + '_ {
        self.animation.steps.iter().map(|step| Op::of(*step))
    }

    /// The steps a jump names, in step order.
    pub fn targets(&self) -> Vec<usize> {
        let mut targets: Vec<usize> = self.animation.steps.iter().filter_map(|step| step.jump()).collect();
        targets.sort_unstable();
        targets.dedup();
        targets
    }

    /// The record's stored bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = self.animation.encode();
        bytes.extend_from_slice(&self.padding);
        bytes
    }

    /// Where the frame-table word of `frame` sits, from the record start.
    pub fn frame_word(&self, frame: u8) -> usize {
        COUNTS_BYTES + STEP_BYTES * self.animation.steps.len() + FRAME_WORD * usize::from(frame)
    }

    /// The bytes, from the record start, that drawing `frame` reads: its frame-table word, then the
    /// cell count and the cells at the offset that word holds. `None` when the word itself is past
    /// the record. A frame past the table reads a word of the frames after it, as the game does.
    pub fn frame_reach(&self, frame: u8) -> Option<core::ops::Range<usize>> {
        let bytes = self.encode();
        let word = self.frame_word(frame);
        let offset = usize::from(u16::from_le_bytes([*bytes.get(word)?, *bytes.get(word + 1)?]));
        let list = self.frame_word(0) + offset;
        let cells = bytes.get(list).map_or(0, |count| usize::from(*count));
        let list_end = list + 1 + CELL_BYTES * cells;
        Some(word.min(list)..list_end.max(word + FRAME_WORD))
    }
}

/// One record: an animation stream, or bytes that do not read as one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Record {
    Stream(Stream),
    /// The stored bytes and why they are not a stream.
    Raw {
        bytes: Vec<u8>,
        reason: String,
    },
}

impl Record {
    /// Reads one record's stored bytes.
    pub fn decode(bytes: &[u8]) -> Self {
        if bytes.is_empty() {
            return Self::Raw { bytes: Vec::new(), reason: "no bytes: the table names the section's end".into() };
        }
        match Animation::decode(bytes) {
            Ok((animation, end)) => Self::Stream(Stream { animation, padding: bytes[end..].to_vec() }),
            Err(error) => Self::Raw { bytes: bytes.to_vec(), reason: error.to_string() },
        }
    }

    /// The record's stored bytes.
    pub fn encode(&self) -> Vec<u8> {
        match self {
            Self::Stream(stream) => stream.encode(),
            Self::Raw { bytes, .. } => bytes.clone(),
        }
    }

    /// The stream, when the record is one.
    pub fn stream(&self) -> Option<&Stream> {
        match self {
            Self::Stream(stream) => Some(stream),
            Self::Raw { .. } => None,
        }
    }
}

/// One section: the record each table entry names, and the records.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Section {
    /// For each table entry, the index of the record it names; entry 0 names record 0.
    pub slots: Vec<u16>,
    pub records: Vec<Record>,
}

/// A step's place: section, record and step index.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StepAt {
    pub section: usize,
    pub record: usize,
    pub step: usize,
}

/// A cell's place: section, record, frame and cell index.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CellAt {
    pub section: usize,
    pub record: usize,
    pub frame: usize,
    pub cell: usize,
}

/// The decode's reach over a set of tables.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Coverage {
    pub tables: usize,
    pub records: usize,
    /// Records that read as a stream.
    pub streams: usize,
    /// Ops (steps) of the streams.
    pub ops: usize,
    /// Ops whose first byte selects an opcode.
    pub known: usize,
    /// First bytes no opcode takes, with their counts.
    pub unknown: BTreeMap<u8, usize>,
    /// Show ops whose frame is past the stream's frame table.
    pub frames_past_table: usize,
}

impl Coverage {
    /// Adds one table.
    pub fn add(&mut self, program: &Program) {
        self.tables += 1;
        for record in program.sections.iter().flat_map(|section| &section.records) {
            self.records += 1;
            let Some(stream) = record.stream() else { continue };
            self.streams += 1;
            for step in &stream.animation.steps {
                self.ops += 1;
                match opcode_of(step.delay) {
                    Some(_) => self.known += 1,
                    None => *self.unknown.entry(step.delay).or_default() += 1,
                }
                if let Op::Show { frame, .. } = Op::of(*step)
                    && usize::from(frame) >= stream.animation.frames.len()
                {
                    self.frames_past_table += 1;
                }
            }
        }
    }

    /// The share of ops with a known opcode, in percent.
    pub fn known_percent(&self) -> f64 {
        if self.ops == 0 { 100.0 } else { 100.0 * self.known as f64 / self.ops as f64 }
    }
}

/// A whole table as programs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Program {
    pub sections: Vec<Section>,
}

impl Program {
    /// Reads a `0x800D3800`, `0x800F0800` or `0x800C3800` payload.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Ok(Self::from_table(&ScriptTable::decode(bytes)?))
    }

    /// The programs of a table read as sections and record bytes.
    pub fn from_table(table: &ScriptTable) -> Self {
        let sections =
            table.sections.iter().map(|section: &ScriptSection| Section { slots: section.slots.clone(), records: section.records.iter().map(|record| Record::decode(record)).collect() }).collect();
        Self { sections }
    }

    /// The payload bytes: the directory, then each section's table words and records. The table
    /// words and the directory follow from the record sizes, so an unchanged program gives the
    /// stored bytes.
    pub fn encode(&self) -> Result<Vec<u8>> {
        if self.sections.is_empty() {
            return Err(Error::ScriptTableShape("a table holds at least one section"));
        }
        let mut sections = Vec::with_capacity(self.sections.len());
        for section in &self.sections {
            sections.push(encode_section(section)?);
        }

        let mut out = Vec::new();
        let mut base = DIRECTORY_WORD * sections.len();
        out.extend_from_slice(&(base as u32).to_le_bytes());
        for section in &sections[..sections.len() - 1] {
            base += section.len();
            out.extend_from_slice(&(base as u32).to_le_bytes());
        }
        for section in sections {
            out.extend(section);
        }
        Ok(out)
    }

    /// The stream at `section`, `record`.
    pub fn stream(&self, section: usize, record: usize) -> Option<&Stream> {
        self.sections.get(section)?.records.get(record)?.stream()
    }

    fn stream_mut(&mut self, section: usize, record: usize) -> Result<&mut Stream> {
        match self.sections.get_mut(section).and_then(|section| section.records.get_mut(record)) {
            Some(Record::Stream(stream)) => Ok(stream),
            Some(Record::Raw { .. }) => Err(refused("the record is not an animation stream")),
            None => Err(refused("no such record")),
        }
    }

    /// Replaces one step's op, in place. A jump must name a step of the stream.
    pub fn set_op(&mut self, at: StepAt, op: Op) -> Result<()> {
        let stream = self.stream_mut(at.section, at.record)?;
        let steps = stream.animation.steps.len();
        let slot = stream.animation.steps.get_mut(at.step).ok_or_else(|| refused("no such step"))?;
        check_target(op, steps)?;
        *slot = op.step();
        Ok(())
    }

    /// Inserts `op` before step `at.step` (or after the last when it equals the step count). Every
    /// jump to a step at or after the insertion moves with it.
    pub fn insert_op(&mut self, at: StepAt, op: Op) -> Result<()> {
        let stream = self.stream_mut(at.section, at.record)?;
        let steps = stream.animation.steps.len();
        if steps >= MAX_STEPS {
            return Err(refused("a stream holds at most 255 steps: the count is one byte"));
        }
        if at.step > steps {
            return Err(refused("no such step"));
        }
        let moved = match op {
            Op::Jump { code, target } if usize::from(target) >= at.step => Op::Jump { code, target: target + 1 },
            op => op,
        };
        check_target(moved, steps + 1)?;

        resized(stream, |animation| {
            for step in &mut animation.steps {
                if step.jump().is_some_and(|target| target >= at.step) {
                    step.frame += 1;
                }
            }
            animation.steps.insert(at.step, moved.step());
        });
        Ok(())
    }

    /// Removes step `at.step`. Refused while a jump names it or when it is the only step; every jump
    /// past it moves back.
    pub fn remove_op(&mut self, at: StepAt) -> Result<()> {
        let stream = self.stream_mut(at.section, at.record)?;
        if at.step >= stream.animation.steps.len() {
            return Err(refused("no such step"));
        }
        if stream.animation.steps.len() == 1 {
            return Err(refused("a stream keeps one step: the step functions stage a step whatever the count"));
        }
        if let Some(from) = stream.animation.steps.iter().enumerate().find(|(index, step)| *index != at.step && step.jump() == Some(at.step)).map(|(index, _)| index) {
            return Err(Error::Import(format!("step {} is the target of the jump at step {from}", at.step)));
        }

        resized(stream, |animation| {
            animation.steps.remove(at.step);
            for step in &mut animation.steps {
                if step.jump().is_some_and(|target| target > at.step) {
                    step.frame -= 1;
                }
            }
        });
        Ok(())
    }

    /// Replaces one cell of one frame, in place.
    pub fn set_cell(&mut self, at: CellAt, cell: Cell) -> Result<()> {
        let stream = self.stream_mut(at.section, at.record)?;
        let slot = stream.animation.frames.get_mut(at.frame).and_then(|frame| frame.cells.get_mut(at.cell)).ok_or_else(|| refused("no such cell"))?;
        *slot = cell;
        Ok(())
    }
}

/// Runs `change` on the stream's animation and sets the padding so the record's stored length keeps
/// its remainder by [`KEPT_ALIGNMENT`]: every record after it keeps its alignment.
fn resized(stream: &mut Stream, change: impl FnOnce(&mut Animation)) {
    let before = stream.encode().len();
    change(&mut stream.animation);
    let body = stream.animation.encode().len();
    let padding = (before as isize - body as isize).rem_euclid(KEPT_ALIGNMENT as isize) as usize;
    stream.padding = vec![0; padding];
}

/// Refuses a jump that names no step of a stream of `steps` steps: the position would land in the
/// frame table.
fn check_target(op: Op, steps: usize) -> Result<()> {
    match op {
        Op::Jump { target, .. } if usize::from(target) >= steps => Err(Error::Import(format!("jump target {target} is past the last step ({})", steps.saturating_sub(1)))),
        _ => Ok(()),
    }
}

fn refused(reason: &str) -> Error {
    Error::Import(reason.to_string())
}

/// One section's bytes: the table words, then the records. Entry k of the table holds the start of
/// the record `slots[k]` names.
fn encode_section(section: &Section) -> Result<Vec<u8>> {
    if section.slots.first() != Some(&0) {
        return Err(Error::ScriptTableShape("a section's first table entry names its first record"));
    }
    if section.slots.iter().any(|slot| usize::from(*slot) >= section.records.len()) {
        return Err(Error::ScriptTableShape("a table entry names no record"));
    }

    let mut records = Vec::with_capacity(section.records.len());
    let mut starts = Vec::with_capacity(section.records.len());
    let mut at = TABLE_WORD * section.slots.len();
    for record in &section.records {
        if at > MAX_RECORD_OFFSET {
            return Err(Error::ScriptTableShape("a record starts past the reach of a u16 table word"));
        }
        if record.stream().is_some() && !at.is_multiple_of(FRAME_WORD) {
            return Err(Error::ScriptTableShape("a stream starts at an odd offset, where its u16 frame words cannot be read"));
        }
        let bytes = record.encode();
        starts.push(at as u16);
        at += bytes.len();
        records.push(bytes);
    }

    let mut out = Vec::with_capacity(at);
    for slot in &section.slots {
        out.extend_from_slice(&starts[usize::from(*slot)].to_le_bytes());
    }
    for record in records {
        out.extend(record);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::Frame;

    fn cell(u: u8) -> Cell {
        Cell { size: 5, x: -8, y: -16, u, v: 0 }
    }

    /// Two frames; steps 0 and 1 show them, step 2 jumps back to step 1.
    fn walk() -> Animation {
        Animation {
            steps: vec![Step { delay: 4, frame: 0 }, Step { delay: 2, frame: 1 }, Step { delay: JUMP, frame: 1 }],
            frames: vec![Frame { cells: vec![cell(0)] }, Frame { cells: vec![cell(16), cell(32)] }],
        }
    }

    /// A table of two sections: the first with a shared record and a padded stream, the second empty.
    fn table() -> Vec<u8> {
        let stream = walk().encode();
        let padded = stream.len() + 1;
        let first = [vec![6u8, 0, 6 + padded as u8, 0, 6 + padded as u8, 0], stream, vec![0], vec![1, 2, 3]].concat();
        let second = vec![2u8, 0];
        let base = 8 + first.len();
        [8u32.to_le_bytes().to_vec(), (base as u32).to_le_bytes().to_vec(), first, second].concat()
    }

    fn at(step: usize) -> StepAt {
        StepAt { section: 0, record: 0, step }
    }

    #[test]
    fn every_byte_value_selects_one_opcode() {
        for code in 0..=u8::MAX {
            let found = OPCODES.iter().filter(|opcode| opcode.codes.contains(&code)).count();
            assert_eq!(found, 1, "{code:#04X}");
        }
    }

    #[test]
    fn a_table_decodes_to_streams_and_encodes_back() {
        let payload = table();
        let program = Program::decode(&payload).expect("decodes");
        assert_eq!(program.sections[0].slots, vec![0, 1, 1]);

        let stream = program.stream(0, 0).expect("a stream");
        assert_eq!(stream.padding, vec![0]);
        assert_eq!(stream.targets(), vec![1]);
        let ops: Vec<Op> = stream.ops().collect();
        assert_eq!(ops[0], Op::Show { ticks: 4, frame: 0 });
        assert_eq!(ops[2], Op::Jump { code: JUMP, target: 1 });

        assert!(matches!(program.sections[0].records[1], Record::Raw { .. }));
        assert!(matches!(&program.sections[1].records[0], Record::Raw { bytes, .. } if bytes.is_empty()));
        assert_eq!(program.encode().expect("encodes"), payload);
    }

    #[test]
    fn an_op_edit_keeps_every_other_byte() {
        let payload = table();
        let mut program = Program::decode(&payload).expect("decodes");
        program.set_op(at(0), Op::Show { ticks: 9, frame: 1 }).expect("edits");
        let edited = program.encode().expect("encodes");
        let changed: Vec<usize> = (0..payload.len()).filter(|index| payload[*index] != edited[*index]).collect();
        assert_eq!(edited.len(), payload.len());
        assert_eq!(changed.len(), 2, "the step's two bytes");
    }

    #[test]
    fn an_insert_moves_the_jumps_after_it() {
        let mut program = Program::decode(&table()).expect("decodes");
        program.insert_op(at(1), Op::Show { ticks: 1, frame: 0 }).expect("inserts");
        let stream = program.stream(0, 0).expect("a stream");
        assert_eq!(stream.animation.steps.len(), 4);
        assert_eq!(stream.animation.steps[3].jump(), Some(2), "the jump still names the old step 1");

        let encoded = program.encode().expect("encodes");
        let again = Program::decode(&encoded).expect("the rebuilt table decodes");
        assert_eq!(again, program);
        assert_eq!(encoded.len() % KEPT_ALIGNMENT, table().len() % KEPT_ALIGNMENT);
    }

    #[test]
    fn a_removal_is_refused_while_a_jump_names_the_step() {
        let mut program = Program::decode(&table()).expect("decodes");
        assert!(program.remove_op(at(1)).is_err());
        program.remove_op(at(0)).expect("step 0 is no target");
        assert_eq!(program.stream(0, 0).expect("a stream").animation.steps[1].jump(), Some(0));
        assert_eq!(Program::decode(&program.encode().expect("encodes")).expect("decodes"), program);
    }

    #[test]
    fn a_jump_past_the_stream_is_refused() {
        let mut program = Program::decode(&table()).expect("decodes");
        assert!(program.set_op(at(0), Op::Jump { code: JUMP, target: 3 }).is_err());
        assert!(program.set_op(at(0), Op::Jump { code: JUMP, target: 2 }).is_ok());
    }

    #[test]
    fn a_frame_reach_covers_its_word_and_cells() {
        let program = Program::decode(&table()).expect("decodes");
        let stream = program.stream(0, 0).expect("a stream");
        let reach = stream.frame_reach(1).expect("inside the record");
        assert_eq!(reach.end, stream.animation.encode().len());
    }
}
