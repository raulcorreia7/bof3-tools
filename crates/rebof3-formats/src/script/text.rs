//! A script bank as text: one op per line, labels for the steps jumps reach.
//!
//! ```text
//! bof3-script 1
//!
//! section 0
//! slots 0 1 1
//!
//! record 0
//! s1:
//!     show frame 0 ticks 4
//!     jump s1
//! frame 0
//!     cell shape 5 x -8 y -16 u 0 v 0
//! pad 00
//!
//! record 1
//! raw 01 02 03
//! ```
//!
//! `section` and `record` lines number their parts in order. `slots` lists the record each table
//! entry names. A record holds steps (`show`, `jump`), then its frames (`frame` and its `cell`
//! lines), then `pad` (the bytes before the next record); or it is `raw` bytes. A label (`name:`)
//! names the next step, and a jump names a label or a step number, so a step added or removed in the
//! text moves every jump with it. `;` starts a comment. A cell's size byte is `shape N` with `flip`
//! for bit 7, or `size 0xNN` when other bits are set. A jump whose first byte is not `0x80` carries
//! `code 0xNN`.
//!
//! [`export`] writes every byte, so [`import`] of an unchanged export gives the stored payload.

use core::fmt::Write as _;
use std::collections::HashMap;

use crate::animation::{Animation, Cell, FLIP, Frame, JUMP, SHAPE_MASK, duration_ms};
use crate::error::{Error, Result};

use super::program::{Op, Program, Record, Section, Stream};

/// The first line of the text form and its version.
const MAGIC: &str = "bof3-script";
const VERSION: &str = "1";
/// Bytes on one `raw` or `pad` line.
const BYTES_PER_LINE: usize = 16;
/// The start of a comment.
const COMMENT: char = ';';
/// The indent of a step or cell line.
const INDENT: &str = "    ";
/// The bits of a cell's size byte the shape and the flip leave: written as `size` when set.
const OTHER_SIZE_BITS: u8 = !(SHAPE_MASK | FLIP);

/// The stored payload as text.
pub fn export(payload: &[u8]) -> Result<Vec<u8>> {
    Ok(write(&Program::decode(payload)?).into_bytes())
}

/// Text from [`export`] back to a payload. The payload may not outgrow `base`, the stored one: what
/// follows the bank in memory is not known to be free.
pub fn import(text: &[u8], base: &[u8]) -> Result<Vec<u8>> {
    let text = core::str::from_utf8(text).map_err(|_| Error::Import("the script text is not UTF-8".into()))?;
    let bytes = parse(text)?.encode()?;
    if bytes.len() > base.len() {
        return Err(Error::Import(format!(
            "the bank grows to {} bytes from {}: what follows it in memory is not known to be free",
            bytes.len(),
            base.len()
        )));
    }
    Ok(bytes)
}

/// The text of a program.
pub fn write(program: &Program) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "; A script bank: one animation stream per record. Actor_BeginScript (exe/slus_004_22@0x8014D6DC)"
    );
    let _ = writeln!(out, "; points an actor at a record and Actor_SeekScript (exe/slus_004_22@0x8014D910) plays it.");
    let _ = writeln!(out, "; show: stage a frame for its ticks (Actor_StageAnimStep; a tick is 33.4 ms, 0 lasts 256).");
    let _ = writeln!(
        out,
        "; jump: continue at a step (Actor_StepAnimLoop, Actor_StepAnimOnce). Labels name the steps jumps reach."
    );
    let _ = writeln!(out, "{MAGIC} {VERSION}");
    for (index, section) in program.sections.iter().enumerate() {
        let _ = writeln!(out, "\nsection {index}");
        let slots: Vec<String> = section.slots.iter().map(u16::to_string).collect();
        let _ = writeln!(out, "slots {}", slots.join(" "));
        for (index, record) in section.records.iter().enumerate() {
            let _ = writeln!(out, "\nrecord {index}");
            match record {
                Record::Stream(stream) => write_stream(&mut out, stream),
                Record::Raw { bytes, reason } => {
                    let _ = writeln!(out, "{COMMENT} not a stream: {reason}");
                    write_bytes(&mut out, "raw", bytes);
                }
            }
        }
    }
    out
}

/// The label of step `index`.
pub fn label(index: usize) -> String {
    format!("s{index}")
}

fn write_stream(out: &mut String, stream: &Stream) {
    let targets = stream.targets();
    let frames = stream.animation.frames.len();
    for (index, op) in stream.ops().enumerate() {
        if targets.contains(&index) {
            let _ = writeln!(out, "{}:", label(index));
        }
        let line = match op {
            Op::Show { ticks, frame } => {
                let past = if usize::from(frame) >= frames { ", past the frame table" } else { "" };
                format!(
                    "show frame {frame} ticks {ticks}{INDENT}{COMMENT} step {index}, {} ms{past}",
                    duration_ms(ticks)
                )
            }
            Op::Jump { code, target } => {
                let code = if code == JUMP { String::new() } else { format!(" code {code:#04X}") };
                format!("jump {}{code}{INDENT}{COMMENT} step {index}", label(usize::from(target)))
            }
        };
        let _ = writeln!(out, "{INDENT}{line}");
    }
    for (index, frame) in stream.animation.frames.iter().enumerate() {
        let _ = writeln!(out, "frame {index}");
        for cell in &frame.cells {
            let size = if cell.size & OTHER_SIZE_BITS == 0 {
                let flip = if cell.flipped() { " flip" } else { "" };
                format!("shape {}{flip}", cell.shape())
            } else {
                format!("size {:#04X}", cell.size)
            };
            let _ = writeln!(out, "{INDENT}cell {size} x {} y {} u {} v {}", cell.x, cell.y, cell.u, cell.v);
        }
    }
    if !stream.padding.is_empty() {
        write_bytes(out, "pad", &stream.padding);
    }
}

fn write_bytes(out: &mut String, keyword: &str, bytes: &[u8]) {
    if bytes.is_empty() {
        let _ = writeln!(out, "{keyword}");
    }
    for line in bytes.chunks(BYTES_PER_LINE) {
        let hex: Vec<String> = line.iter().map(|byte| format!("{byte:02x}")).collect();
        let _ = writeln!(out, "{keyword} {}", hex.join(" "));
    }
}

/// The record a parse is building.
#[derive(Default)]
struct Draft {
    raw: Option<Vec<u8>>,
    /// Each step's first byte and its jump target or frame, unresolved.
    steps: Vec<(u8, Operand)>,
    frames: Vec<Frame>,
    padding: Vec<u8>,
    labels: HashMap<String, usize>,
    pending: Vec<String>,
}

/// A step's second byte as written: a frame, or a jump target by label or number.
enum Operand {
    Frame(u8),
    Label(String, usize),
    Step(u8),
}

impl Draft {
    fn is_stream(&self) -> bool {
        !self.steps.is_empty() || !self.frames.is_empty() || !self.padding.is_empty()
    }

    fn finish(self, line: usize) -> Result<Record> {
        if !self.pending.is_empty() {
            return Err(at(line, "a label names no step"));
        }
        match self.raw {
            Some(bytes) if !self.is_stream() => Ok(Record::Raw { bytes, reason: "raw in the text".into() }),
            Some(_) => Err(at(line, "a record is raw or a stream, not both")),
            None if self.steps.is_empty() => Err(at(line, "a stream holds at least one step; write `raw` for bytes")),
            None => {
                let mut steps = Vec::with_capacity(self.steps.len());
                for (code, operand) in self.steps {
                    let frame = match operand {
                        Operand::Frame(frame) | Operand::Step(frame) => frame,
                        Operand::Label(name, line) => {
                            let index = *self.labels.get(&name).ok_or_else(|| at(line, "the jump names no label"))?;
                            u8::try_from(index).map_err(|_| at(line, "a jump target is past step 255"))?
                        }
                    };
                    steps.push(crate::animation::Step { delay: code, frame });
                }
                let animation = Animation { steps, frames: self.frames };
                if animation.steps.len() > super::program::MAX_STEPS || animation.frames.len() > u8::MAX as usize {
                    return Err(at(line, "a stream holds at most 255 steps and 255 frames"));
                }
                Ok(Record::Stream(Stream { animation, padding: self.padding }))
            }
        }
    }
}

/// Reads the text form back to a program.
pub fn parse(text: &str) -> Result<Program> {
    let mut lines =
        text.lines().enumerate().map(|(index, line)| (index + 1, strip(line))).filter(|(_, line)| !line.is_empty());
    match lines.next() {
        Some((_, line)) if line.split_whitespace().eq([MAGIC, VERSION]) => {}
        _ => return Err(Error::Import(format!("the script text does not start with `{MAGIC} {VERSION}`"))),
    }

    let mut sections: Vec<Section> = Vec::new();
    let mut draft: Option<Draft> = None;
    let mut last = 0;
    for (line, content) in lines {
        last = line;
        let words: Vec<&str> = content.split_whitespace().collect();
        let (keyword, rest) = (words[0], &words[1..]);

        if let Some(name) = keyword.strip_suffix(':').filter(|_| rest.is_empty()) {
            let record = draft.as_mut().ok_or_else(|| at(line, "a label outside a record"))?;
            record.pending.push(name.to_string());
            continue;
        }

        match keyword {
            "section" => {
                close(&mut sections, &mut draft, line)?;
                expect_index(rest, sections.len(), line, "section")?;
                sections.push(Section { slots: Vec::new(), records: Vec::new() });
            }
            "slots" => {
                let section = sections.last_mut().ok_or_else(|| at(line, "slots outside a section"))?;
                section.slots = rest.iter().map(|word| number(word, line)).collect::<Result<_>>()?;
            }
            "record" => {
                close(&mut sections, &mut draft, line)?;
                let section = sections.last().ok_or_else(|| at(line, "a record outside a section"))?;
                expect_index(rest, section.records.len(), line, "record")?;
                draft = Some(Draft::default());
            }
            _ => {
                let record = draft.as_mut().ok_or_else(|| at(line, "an op outside a record"))?;
                record_line(record, keyword, rest, line)?;
            }
        }
    }
    close(&mut sections, &mut draft, last)?;
    Ok(Program { sections })
}

/// Ends the record being built and adds it to the last section.
fn close(sections: &mut [Section], draft: &mut Option<Draft>, line: usize) -> Result<()> {
    if let Some(record) = draft.take() {
        let section = sections.last_mut().ok_or_else(|| at(line, "a record outside a section"))?;
        section.records.push(record.finish(line)?);
    }
    Ok(())
}

/// One line inside a record.
fn record_line(record: &mut Draft, keyword: &str, rest: &[&str], line: usize) -> Result<()> {
    match keyword {
        "show" | "jump" if !record.frames.is_empty() || !record.padding.is_empty() => {
            return Err(at(line, "a step after the frames"));
        }
        "show" => {
            let fields = Fields::read(rest, &[], line)?;
            let ticks: u8 = fields.number("ticks")?;
            if ticks >= JUMP {
                return Err(at(line, "ticks run from 0 to 127; 128 and up make the step a jump"));
            }
            record.steps.push((ticks, Operand::Frame(fields.number("frame")?)));
        }
        "jump" => {
            let (target, rest) = rest.split_first().ok_or_else(|| at(line, "a jump names its target"))?;
            let fields = Fields::read(rest, &[], line)?;
            let code: u8 = fields.optional("code")?.unwrap_or(JUMP);
            if code < JUMP {
                return Err(at(line, "a jump's code runs from 0x80 to 0xFF"));
            }
            let operand = match parse_number::<u8>(target) {
                Some(step) => Operand::Step(step),
                None => Operand::Label((*target).to_string(), line),
            };
            record.steps.push((code, operand));
        }
        "frame" => {
            if !record.padding.is_empty() {
                return Err(at(line, "a frame after the padding"));
            }
            expect_index(rest, record.frames.len(), line, "frame")?;
            record.frames.push(Frame::default());
        }
        "cell" => {
            let frame = record.frames.last_mut().ok_or_else(|| at(line, "a cell outside a frame"))?;
            let fields = Fields::read(rest, &["flip"], line)?;
            let size = match fields.optional::<u8>("size")? {
                Some(size) => size,
                None => {
                    let shape: u8 = fields.number("shape")?;
                    if shape > SHAPE_MASK {
                        return Err(at(line, "a shape runs from 0 to 15"));
                    }
                    shape | if fields.flag("flip") { FLIP } else { 0 }
                }
            };
            frame.cells.push(Cell {
                size,
                x: fields.number("x")?,
                y: fields.number("y")?,
                u: fields.number("u")?,
                v: fields.number("v")?,
            });
        }
        "pad" => record.padding.extend(hex(rest, line)?),
        "raw" => record.raw.get_or_insert_with(Vec::new).extend(hex(rest, line)?),
        _ => return Err(at(line, &format!("`{keyword}` is not an op"))),
    }

    if matches!(keyword, "show" | "jump") {
        let index = record.steps.len() - 1;
        for name in record.pending.drain(..) {
            if record.labels.insert(name, index).is_some() {
                return Err(at(line, "a label names two steps"));
            }
        }
    } else if !record.pending.is_empty() {
        return Err(at(line, "a label names no step"));
    }
    Ok(())
}

/// The `key value` pairs and flags after a keyword.
struct Fields<'a> {
    values: HashMap<&'a str, &'a str>,
    flags: Vec<&'a str>,
    line: usize,
}

impl<'a> Fields<'a> {
    fn read(words: &[&'a str], flags: &[&str], line: usize) -> Result<Self> {
        let (mut values, mut set) = (HashMap::new(), Vec::new());
        let mut words = words.iter();
        while let Some(word) = words.next() {
            if flags.contains(word) {
                set.push(*word);
                continue;
            }
            let value = words.next().ok_or_else(|| at(line, &format!("`{word}` has no value")))?;
            if values.insert(*word, *value).is_some() {
                return Err(at(line, &format!("`{word}` is given twice")));
            }
        }
        Ok(Self { values, flags: set, line })
    }

    fn optional<T: TryFrom<i64>>(&self, key: &str) -> Result<Option<T>> {
        self.values.get(key).map(|word| number(word, self.line)).transpose()
    }

    fn number<T: TryFrom<i64>>(&self, key: &str) -> Result<T> {
        self.optional(key)?.ok_or_else(|| at(self.line, &format!("`{key}` is missing")))
    }

    fn flag(&self, name: &str) -> bool {
        self.flags.contains(&name)
    }
}

/// A decimal or `0x` number that fits `T`.
fn parse_number<T: TryFrom<i64>>(word: &str) -> Option<T> {
    let (negative, digits) = match word.strip_prefix('-') {
        Some(digits) => (true, digits),
        None => (false, word),
    };
    let value = match digits.strip_prefix("0x").or_else(|| digits.strip_prefix("0X")) {
        Some(hex) => i64::from_str_radix(hex, 16).ok()?,
        None => digits.parse::<i64>().ok()?,
    };
    T::try_from(if negative { -value } else { value }).ok()
}

fn number<T: TryFrom<i64>>(word: &str, line: usize) -> Result<T> {
    parse_number(word).ok_or_else(|| at(line, &format!("`{word}` is not a number in range")))
}

fn hex(words: &[&str], line: usize) -> Result<Vec<u8>> {
    words
        .iter()
        .map(|word| u8::from_str_radix(word, 16).map_err(|_| at(line, &format!("`{word}` is not a hex byte"))))
        .collect()
}

fn expect_index(rest: &[&str], expected: usize, line: usize, part: &str) -> Result<()> {
    match rest {
        [word] if parse_number::<usize>(word) == Some(expected) => Ok(()),
        _ => Err(at(line, &format!("expected `{part} {expected}`"))),
    }
}

/// The line without its comment and surrounding space.
fn strip(line: &str) -> &str {
    line.split_once(COMMENT).map_or(line, |(code, _)| code).trim()
}

fn at(line: usize, message: &str) -> Error {
    Error::Import(format!("script text line {line}: {message}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::Step;

    /// A bank: one section of a stream with a loop and a pad, a shared table entry and a raw record.
    fn bank() -> Vec<u8> {
        let animation = Animation {
            steps: vec![Step { delay: 4, frame: 0 }, Step { delay: 0, frame: 1 }, Step { delay: 0x81, frame: 1 }],
            frames: vec![
                Frame { cells: vec![Cell { size: 0x85, x: -8, y: -16, u: 0, v: 0 }] },
                Frame { cells: vec![Cell { size: 0x13, x: 4, y: 2, u: 9, v: 7 }] },
            ],
        };
        let stream = [animation.encode(), vec![0, 0]].concat();
        let start = 6 + stream.len() as u16;
        let table = [6u16, start, start].iter().flat_map(|word| word.to_le_bytes()).collect::<Vec<_>>();
        [4u32.to_le_bytes().to_vec(), table, stream, vec![0xAA, 0xBB, 0xCC]].concat()
    }

    #[test]
    fn an_unchanged_export_imports_to_the_stored_bytes() {
        let payload = bank();
        let text = export(&payload).expect("exports");
        assert_eq!(import(&text, &payload).expect("imports"), payload);
    }

    #[test]
    fn the_text_names_ops_labels_and_cells() {
        let text = String::from_utf8(export(&bank()).expect("exports")).expect("UTF-8");
        assert!(text.contains("s1:\n    show frame 1 ticks 0"), "{text}");
        assert!(text.contains("jump s1 code 0x81"), "{text}");
        assert!(text.contains("cell shape 5 flip x -8 y -16 u 0 v 0"), "{text}");
        assert!(text.contains("cell size 0x13 x 4 y 2 u 9 v 7"), "{text}");
        assert!(text.contains("raw aa bb cc"), "{text}");
    }

    #[test]
    fn an_added_step_moves_the_jump_through_its_label() {
        let payload = bank();
        let text = String::from_utf8(export(&payload).expect("exports")).expect("UTF-8");
        let grown = text
            .replacen("    show frame 0 ticks 4", "    show frame 0 ticks 4\n    show frame 0 ticks 2", 1)
            .replacen("pad 00 00\n", "", 1);
        let program = parse(&grown).expect("parses");
        let stream = program.stream(0, 0).expect("a stream");
        assert_eq!(stream.animation.steps[3], Step { delay: 0x81, frame: 2 });
        assert_eq!(
            import(grown.as_bytes(), &payload).expect("imports").len(),
            payload.len(),
            "the new step takes the pad"
        );
    }

    #[test]
    fn a_bank_that_outgrows_its_payload_is_refused() {
        let payload = bank();
        let text = String::from_utf8(export(&payload).expect("exports")).expect("UTF-8");
        let grown = text.replacen("pad 00 00", "pad 00 00 00 00", 1);
        assert!(import(grown.as_bytes(), &payload).is_err());
    }

    #[test]
    fn a_broken_line_names_its_number() {
        let text = format!("{MAGIC} {VERSION}\nsection 0\nslots 0\nrecord 0\n    show frame 0\n");
        let error = parse(&text).expect_err("ticks are missing").to_string();
        assert!(error.contains("line 5") && error.contains("ticks"), "{error}");
        assert!(parse("section 0").is_err(), "no header");
        assert!(parse(&format!("{MAGIC} {VERSION}\nsection 0\nslots 0\nrecord 0\n    jump nowhere\n")).is_err());
    }
}
