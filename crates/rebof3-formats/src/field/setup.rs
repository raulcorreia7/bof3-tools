//! The setup stream: the bytes `Area_RunSetupStream` (`emi/etc/game/00@0x801A3E50`) runs when an area
//! opens, which place its actors.
//!
//! An area's `AreaLoad` record (`g_AreaLoads[area]`, [`super::Executable::area_load`]) starts with the
//! address of its stream, inside the area program. `func_801A3F3C` reads it to `0xFF`:
//!
//! | Byte | Operands | Meaning | Reader |
//! | ---- | -------- | ------- | ------ |
//! | `< 0xF0` | the record's bytes | an actor record; the high nibble is its kind | `func_801A4BF8` |
//! | `F0 tt pp` | then-body [`FD` else-body] `FE` | run the then-body when test `tt & 0x1F` of `pp` holds | `func_801A3FC0` |
//! | `F1 tt pp` | the same | the test inverted | `func_801A4158` |
//! | `F4 tt` | { `F6 vv` \| `F7` } body `F8` ... `F5` | a switch on test `tt`: case `vv`, default, break, end | `func_801A4518` |
//! | `F9 vv` | | sets the flag row test 2 reads (`D_8014820C`) | `func_801A4400` |
//! | `FA` | | sets that row to the scenario | `func_801A4400` |
//!
//! A case body falls through later labels until `F8`, as C's does (`func_801A4640`). Any other byte
//! where a record or block is expected hangs the game, so [`walk`] refuses it. The record lengths are
//! `s_ScriptOpcodeLengths` (`emi/etc/game/00@0x801C8444`), indexed by kind.
//!
//! A record's bytes are edited in place: no edit changes a length, so every other byte of the program
//! stays as it was.

use crate::error::{Error, Result};

/// The end of a stream.
pub const END: u8 = 0xFF;
const IF: u8 = 0xF0;
const IF_NOT: u8 = 0xF1;
const SWITCH: u8 = 0xF4;
const SWITCH_END: u8 = 0xF5;
const CASE: u8 = 0xF6;
const DEFAULT: u8 = 0xF7;
const BREAK: u8 = 0xF8;
const SET_ROW: u8 = 0xF9;
const SET_ROW_SCENARIO: u8 = 0xFA;
const ELSE: u8 = 0xFD;
const END_IF: u8 = 0xFE;
/// The first block byte; a smaller byte starts a record.
const FIRST_BLOCK: u8 = 0xF0;
/// The bits of an `F0`/`F1` test id (`func_801A3FC0`: `tt & 0x1F`).
const TEST_MASK: u8 = 0x1F;

/// Where one kind of record keeps its fields, from its handler.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Layout {
    /// `s_ScriptOpcodeLengths[kind]`.
    pub length: usize,
    /// The handler `func_801A4BF8` calls.
    pub handler: &'static str,
    /// Whether bytes 1 and 2 are a big-endian scene id, looked up by `Area_FindSceneRecord`
    /// (`exe/slus_004_22@0x8014DD3C`).
    pub scene: bool,
    /// The bytes of `x` half, `x` whole, `y` half and `y` whole.
    pub position: Option<[usize; 4]>,
    /// Whether the low nibble of byte 0 is the facing (`direction_08 = script[0] & 0xF`).
    pub facing: bool,
}

const fn layout(
    length: usize,
    handler: &'static str,
    scene: bool,
    position: Option<[usize; 4]>,
    facing: bool,
) -> Layout {
    Layout { length, handler, scene, position, facing }
}

/// The record layouts, by kind (the high nibble of byte 0). Every handler is in `emi/etc/game/00`.
pub const KINDS: [Layout; 16] = [
    layout(0x11, "func_801A4D54", true, Some([6, 7, 8, 9]), true),
    layout(0x10, "func_801A50C0", true, Some([6, 7, 8, 9]), true),
    layout(0x13, "func_801A57F0", true, Some([6, 7, 8, 9]), true),
    layout(0x12, "func_801A5B78", true, Some([6, 7, 8, 9]), true),
    layout(0x11, "func_801A5EFC", true, Some([6, 7, 8, 9]), true),
    layout(0x12, "func_801A6F58", true, Some([5, 6, 7, 8]), true),
    layout(0x10, "func_801A65A0 (party member)", false, Some([4, 5, 6, 7]), true),
    layout(0x11, "func_801A623C", true, Some([5, 6, 7, 8]), true),
    layout(0x0C, "func_801A5400 (blocks its cells)", true, Some([4, 5, 6, 7]), true),
    layout(0x0D, "func_801A7330", true, Some([4, 5, 6, 7]), true),
    layout(0x0E, "func_801A68C0 (AreaLoad object)", false, Some([1, 2, 3, 4]), true),
    layout(0x0E, "func_801A6C98 (AreaLoad object)", false, Some([1, 2, 3, 4]), true),
    layout(0x02, "func_801A72B0", false, None, false),
    layout(0x04, "func_801A72E8 (changes the last actor)", false, None, false),
    layout(0x07, "Actor_Create (scene object)", false, Some([2, 3, 4, 5]), false),
    layout(0x00, "none", false, None, false),
];

/// What a record runs under: the blocks around it, outermost first.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Guard {
    /// An `F0` or `F1` block: its test, operand, polarity and the branch the record is in.
    If { at: usize, test: u8, operand: u8, negated: bool, otherwise: bool },
    /// A switch case (`value`) or its default (`None`).
    Case { at: usize, test: u8, value: Option<u8> },
}

impl Guard {
    /// A short text: `if t02(05)`, `else of if t02(05)`, `case t08 = 3`.
    pub fn label(&self) -> String {
        match *self {
            Guard::If { test, operand, negated, otherwise, .. } => {
                let polarity = if negated { "if not" } else { "if" };
                let branch = if otherwise { "else of " } else { "" };
                format!("{branch}{polarity} test {test} ({operand:#04X})")
            }
            Guard::Case { test, value: Some(value), .. } => format!("case test {test} = {value:#04X}"),
            Guard::Case { test, value: None, .. } => format!("default of test {test}"),
        }
    }
}

/// One actor record of a stream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Record {
    /// The record's offset in the program.
    pub at: usize,
    pub kind: u8,
    pub guards: Vec<Guard>,
}

impl Record {
    pub fn layout(&self) -> &'static Layout {
        &KINDS[usize::from(self.kind & 0xF)]
    }

    /// The record's bytes in `program`.
    pub fn bytes<'a>(&self, program: &'a [u8]) -> &'a [u8] {
        &program[self.at..self.at + self.layout().length]
    }

    /// Where the record puts its actor, when its kind has a position.
    pub fn placement(&self, program: &[u8]) -> Option<Placement> {
        let layout = self.layout();
        let [x_half, x_whole, y_half, y_whole] = layout.position?;
        let bytes = self.bytes(program);
        let halves = |half: usize, whole: usize| 2 * u16::from(bytes[whole]) + u16::from(bytes[half] != 0);
        Some(Placement {
            x2: halves(x_half, x_whole),
            y2: halves(y_half, y_whole),
            facing: layout.facing.then_some(bytes[0] & 0xF),
            scene: layout.scene.then(|| u16::from_be_bytes([bytes[1], bytes[2]])),
        })
    }
}

/// Where a record places its actor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Placement {
    /// The position in half cells: `x2 / 2` is the cell column, and an odd `x2` is half a cell on.
    pub x2: u16,
    pub y2: u16,
    /// The facing, a row of `g_FacingSteps` for `0` to `7`.
    pub facing: Option<u8>,
    /// The scene id whose record gives the sprite's page, CLUT row and script section.
    pub scene: Option<u16>,
}

/// A walked stream.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Stream {
    /// The offset of the stream's first byte in the program.
    pub start: usize,
    /// The offset of its `0xFF`.
    pub end: usize,
    pub records: Vec<Record>,
}

/// Walks the stream at `start` of `program` to its `0xFF`.
pub fn walk(program: &[u8], start: usize) -> Result<Stream> {
    let mut walker = Walker { program, records: Vec::new() };
    let end = walker.stream(start)?;
    Ok(Stream { start, end, records: walker.records })
}

/// The program with `record` moved to `x2`, `y2` half cells.
pub fn with_position(program: &[u8], record: &Record, x2: u16, y2: u16) -> Result<Vec<u8>> {
    let [x_half, x_whole, y_half, y_whole] = record
        .layout()
        .position
        .ok_or_else(|| Error::Field(format!("a kind {:X} record has no position", record.kind)))?;
    let (x, y) = (x2 / 2, y2 / 2);
    let (Ok(x), Ok(y)) = (u8::try_from(x), u8::try_from(y)) else {
        return Err(Error::Field(format!("cell ({x}, {y}) does not fit the record's bytes")));
    };
    let mut program = program.to_vec();
    let bytes = &mut program[record.at..record.at + record.layout().length];
    for (half, whole, cell, odd) in [(x_half, x_whole, x, x2 & 1 == 1), (y_half, y_whole, y, y2 & 1 == 1)] {
        bytes[whole] = cell;
        if odd != (bytes[half] != 0) {
            bytes[half] = u8::from(odd);
        }
    }
    Ok(program)
}

/// The program with `record` facing `facing` (`0` to `7`, the rows of `g_FacingSteps`).
pub fn with_facing(program: &[u8], record: &Record, facing: u8) -> Result<Vec<u8>> {
    if !record.layout().facing {
        return Err(Error::Field(format!("a kind {:X} record has no facing", record.kind)));
    }
    if usize::from(facing) >= super::FACINGS {
        return Err(Error::Field(format!("facing {facing} is past the {} rows of g_FacingSteps", super::FACINGS)));
    }
    let mut program = program.to_vec();
    program[record.at] = (record.kind << 4) | facing;
    Ok(program)
}

/// The program with `record` naming scene id `scene`, which selects the actor's sprite page, CLUT row and script
pub fn with_scene(program: &[u8], record: &Record, scene: u16) -> Result<Vec<u8>> {
    if !record.layout().scene {
        return Err(Error::Field(format!("a kind {:X} record names no scene", record.kind)));
    }
    let mut program = program.to_vec();
    program[record.at + 1..record.at + 3].copy_from_slice(&scene.to_be_bytes());
    Ok(program)
}

struct Walker<'a> {
    program: &'a [u8],
    records: Vec<Record>,
}

impl Walker<'_> {
    fn byte(&self, at: usize) -> Result<u8> {
        self.program
            .get(at)
            .copied()
            .ok_or_else(|| Error::Field(format!("the setup stream runs past the program at {at:#X}")))
    }

    fn hang(&self, at: usize, place: &str) -> Error {
        Error::Field(format!(
            "byte {:#04X} at {at:#X} {place}; the game would hang there",
            self.program.get(at).copied().unwrap_or(0)
        ))
    }

    fn stream(&mut self, mut at: usize) -> Result<usize> {
        loop {
            if self.byte(at)? == END {
                return Ok(at);
            }
            at = self.element(at, &[])?.ok_or_else(|| self.hang(at, "is not a record or a block"))?;
        }
    }

    /// One record or block; `None` for a byte that is neither.
    fn element(&mut self, at: usize, guards: &[Guard]) -> Result<Option<usize>> {
        let byte = self.byte(at)?;
        if byte < FIRST_BLOCK {
            let kind = byte >> 4;
            let length = KINDS[usize::from(kind)].length;
            if length == 0 || at + length > self.program.len() {
                return Err(Error::Field(format!("the kind {kind:X} record at {at:#X} runs past the program")));
            }
            self.records.push(Record { at, kind, guards: guards.to_vec() });
            return Ok(Some(at + length));
        }
        match byte {
            IF | IF_NOT => {
                let (test, operand) = (self.byte(at + 1)? & TEST_MASK, self.byte(at + 2)?);
                let guard = |otherwise| Guard::If { at, test, operand, negated: byte == IF_NOT, otherwise };
                self.if_body(at + 3, guards, guard).map(Some)
            }
            SWITCH => {
                let test = self.byte(at + 1)?;
                self.switch(at + 2, guards, at, test).map(Some)
            }
            SET_ROW => self.byte(at + 1).map(|_| Some(at + 2)),
            SET_ROW_SCENARIO => Ok(Some(at + 1)),
            _ => Ok(None),
        }
    }

    fn if_body(&mut self, mut at: usize, guards: &[Guard], guard: impl Fn(bool) -> Guard) -> Result<usize> {
        let mut inner = guards.to_vec();
        inner.push(guard(false));
        loop {
            match self.byte(at)? {
                END_IF => return Ok(at + 1),
                ELSE => {
                    *inner.last_mut().expect("the block's guard") = guard(true);
                    at += 1;
                }
                // A no-op in a running body (`func_801A3FC0`).
                END => at += 1,
                _ => at = self.element(at, &inner)?.ok_or_else(|| self.hang(at, "is not allowed in an if body"))?,
            }
        }
    }

    fn switch(&mut self, mut at: usize, guards: &[Guard], start: usize, test: u8) -> Result<usize> {
        loop {
            let mut label = match self.byte(at)? {
                SWITCH_END => return Ok(at + 1),
                CASE => {
                    at += 2;
                    Guard::Case { at: start, test, value: Some(self.byte(at - 1)?) }
                }
                DEFAULT => {
                    at += 1;
                    Guard::Case { at: start, test, value: None }
                }
                _ => return Err(self.hang(at, "is not a case, a default or the switch's end")),
            };
            loop {
                match self.byte(at)? {
                    BREAK => {
                        at += 1;
                        break;
                    }
                    CASE => {
                        label = Guard::Case { at: start, test, value: Some(self.byte(at + 1)?) };
                        at += 2;
                    }
                    DEFAULT => {
                        label = Guard::Case { at: start, test, value: None };
                        at += 1;
                    }
                    _ => {
                        let mut inner = guards.to_vec();
                        inner.push(label);
                        at = self.element(at, &inner)?.ok_or_else(|| self.hang(at, "is not allowed in a case body"))?;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A kind 0 record at cell (3.5, 4) facing 2, then an `if` with a kind 9 record in its else branch, then a
    fn program() -> Vec<u8> {
        let mut bytes = vec![0x02, 0x00, 0x07, 0, 0, 0, 1, 3, 0, 4, 0, 0, 0, 0, 0, 0, 0];
        bytes.extend([IF, 0x22, 5, ELSE, 0x93, 0, 7, 0, 0, 5, 0, 6, 0, 0, 0, 0, 0, END_IF]);
        bytes.extend([SWITCH, 8, CASE, 1, 0xE0, 1, 0, 9, 0, 8, 0, BREAK, SWITCH_END, END]);
        bytes
    }

    #[test]
    fn a_stream_lists_its_records_with_their_guards() {
        let program = program();
        let stream = walk(&program, 0).expect("the stream walks");
        assert_eq!(stream.end, program.len() - 1);
        let kinds: Vec<u8> = stream.records.iter().map(|record| record.kind).collect();
        assert_eq!(kinds, [0, 9, 0xE]);
        let placement = stream.records[0].placement(&program).expect("a position");
        assert_eq!(placement, Placement { x2: 7, y2: 8, facing: Some(2), scene: Some(7) });
        assert_eq!(
            stream.records[1].guards,
            [Guard::If { at: 17, test: 2, operand: 5, negated: false, otherwise: true }]
        );
        assert_eq!(stream.records[2].guards, [Guard::Case { at: 35, test: 8, value: Some(1) }]);
        assert_eq!(stream.records[2].placement(&program).unwrap().facing, None);
    }

    #[test]
    fn a_position_and_a_facing_edit_their_bytes_alone() {
        let program = program();
        let record = &walk(&program, 0).unwrap().records[0];
        assert_eq!(
            with_position(&program, record, 7, 8).unwrap(),
            program,
            "an unchanged position writes the same bytes"
        );
        let moved = with_position(&program, record, 10, 9).unwrap();
        let changed: Vec<usize> = (0..program.len()).filter(|&at| moved[at] != program[at]).collect();
        assert_eq!(changed, [6, 7, 8], "x half, x whole and y half");
        assert_eq!(record.placement(&moved).unwrap().x2, 10);
        let turned = with_facing(&program, record, 5).unwrap();
        assert_eq!(turned[0], 0x05);
        assert!(with_facing(&program, record, 8).is_err());
        assert!(with_position(&program, record, 600, 0).is_err());
        let renamed = with_scene(&program, record, 0x0102).unwrap();
        assert_eq!(record.placement(&renamed).unwrap().scene, Some(0x0102));
        assert_eq!(with_scene(&program, record, 7).unwrap(), program, "an unchanged id writes the same bytes");
        let kind_e = &walk(&program, 0).unwrap().records[2];
        assert!(with_scene(&program, kind_e, 1).is_err());
    }

    #[test]
    fn a_byte_the_game_hangs_on_is_refused() {
        assert!(walk(&[0xF2, END], 0).is_err());
        assert!(walk(&[SWITCH, 0, 0x00], 0).is_err());
        assert!(walk(&[0x00, 0x01], 0).is_err(), "a record that runs past the program");
    }
}
