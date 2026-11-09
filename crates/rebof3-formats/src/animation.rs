//! An animation: the step stream an actor plays and the frames it shows.
//!
//! Two formats hold animations, so this module is shared. Work `+0x50` points at one from either
//! source: `Actor_SelectScriptBank` (`exe/slus_004_22@0x8014DC08`) takes `table + table[n]` of a
//! character sprites section's animation table, and `Actor_BeginScript` (`exe/slus_004_22@0x8014D6DC`) takes
//! a record of the script table `g_ScriptBank` names. `Actor_SeekScript`
//! (`exe/slus_004_22@0x8014D910`) then reads it.
//!
//! | Offset | Size | Field | Meaning |
//! | -----: | ---: | ----- | ------- |
//! | `0x00` | 1 | `frames` | entries in the frame table; no reader takes it, and it equals the table's length in every retail animation |
//! | `0x01` | 1 | `steps` | step count, staged at work `+0x49` |
//! | `0x02` | 2 × `steps` | `step` | `delay, frame` pairs |
//! | - | 2 × `frames` | `frame_offset` | frame offsets from the start of this table, staged at work `+0x54` |
//! | - | - | `frame` | a cell count, then that many 5-byte cells |
//!
//! `Actor_StageAnimStep` stages a step: the delay byte goes to work `+0x4A` and the frame's offset to
//! work `+0x5A`, which `Area_DrawObjectSprite` (`exe/slus_004_22@0x8014C4D4`) draws. A step whose
//! delay byte is [`JUMP`] or more is a jump: its second byte is the step to continue at. The three
//! step functions are in [`Player`].
//!
//! A cell is the `SpriteCell` that `Area_DrawObjectSprite` and `Area_DrawSpriteCell` read.
//!
//! | Offset | Size | Field | Meaning |
//! | -----: | ---: | ----- | ------- |
//! | `0x00` | 1 | `size` | bits 0-3 index `g_SpriteSizes`; bit 7 draws the cell flipped left to right |
//! | `0x01` | 1 | `x` | signed offset from the actor's screen position |
//! | `0x02` | 1 | `y` | signed offset from the actor's screen position |
//! | `0x03` | 1 | `u` | texel column in the frame sheet |
//! | `0x04` | 1 | `v` | texel row in the frame sheet, added to the actor's sheet row (work `+0x26`) |

use crate::error::{Error, Result};

/// A delay byte at or above this makes the step a jump (`entry[0] >= 0x80` in `Actor_StepAnimLoop`).
pub const JUMP: u8 = 0x80;
/// `SYS_Main` (`exe/slus_004_22@0x8014AAC8`) runs one game frame per `VSync(2)`, and a step function runs once a
const FIELDS_PER_TICK: f64 = 2.0;
const MICROS_PER_FIELD: f64 = 1_000_000.0 * 1001.0 / 60_000.0;
/// One tick in microseconds.
pub const TICK_MICROS: f64 = FIELDS_PER_TICK * MICROS_PER_FIELD;
const MICROS_PER_MILLI: f64 = 1000.0;
/// The ticks a delay byte of 0 lasts: the step function counts the byte down before it tests it.
pub const WRAPPED_TICKS: u32 = 256;
/// Bytes of the two counts that open an animation.
const COUNTS_BYTES: usize = 2;
/// Bytes of one step and of one frame offset.
const STEP_BYTES: usize = 2;
const OFFSET_BYTES: usize = 2;
/// Bytes of one cell.
pub const CELL_BYTES: usize = 5;
/// The bits of a cell's size byte that index `g_SpriteSizes` (`GFX_SPRITE_SIZE_MASK`).
pub const SHAPE_MASK: u8 = 0x0F;
/// The bit of a cell's size byte that flips it (`mode = cell->size >> 7`).
pub const FLIP: u8 = 0x80;

/// The ticks a step with `delay` shows its frame.
pub fn ticks_of(delay: u8) -> u32 {
    if delay == 0 { WRAPPED_TICKS } else { u32::from(delay) }
}

/// A step's time in whole milliseconds.
pub fn duration_ms(delay: u8) -> u32 {
    (f64::from(ticks_of(delay)) * TICK_MICROS / MICROS_PER_MILLI).round() as u32
}

/// One step of the stream: show `frame` for `delay` ticks, or a jump.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Step {
    /// Ticks the frame stays, or [`JUMP`] and up for a jump.
    pub delay: u8,
    /// The frame to show, or the step a jump continues at.
    pub frame: u8,
}

impl Step {
    /// The step a jump continues at.
    pub fn jump(self) -> Option<usize> {
        (self.delay >= JUMP).then_some(usize::from(self.frame))
    }
}

/// One cell: a rectangle of the frame sheet drawn at an offset from the actor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cell {
    /// The size byte: the shape in bits 0-3, [`FLIP`] in bit 7.
    pub size: u8,
    /// Screen offset from the actor.
    pub x: i8,
    pub y: i8,
    /// Texel position in the frame sheet.
    pub u: u8,
    pub v: u8,
}

impl Cell {
    /// The index into `g_SpriteSizes`.
    pub fn shape(self) -> usize {
        usize::from(self.size & SHAPE_MASK)
    }

    /// Whether the cell is drawn flipped left to right.
    pub fn flipped(self) -> bool {
        self.size & FLIP != 0
    }

    fn bytes(self) -> [u8; CELL_BYTES] {
        [self.size, self.x as u8, self.y as u8, self.u, self.v]
    }
}

/// One frame: its cells in draw order, the last on top.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Frame {
    pub cells: Vec<Cell>,
}

/// One animation of a section.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Animation {
    pub steps: Vec<Step>,
    pub frames: Vec<Frame>,
}

impl Animation {
    /// The animation at the start of `bytes`, and the bytes it spans.
    pub fn decode(bytes: &[u8]) -> Result<(Self, usize)> {
        let byte = |at: usize| bytes.get(at).copied().ok_or_else(|| short(at));
        let (frame_count, step_count) = (usize::from(byte(0)?), usize::from(byte(1)?));
        let steps = (0..step_count)
            .map(|index| {
                let at = COUNTS_BYTES + STEP_BYTES * index;
                Ok(Step { delay: byte(at)?, frame: byte(at + 1)? })
            })
            .collect::<Result<Vec<_>>>()?;

        let table = COUNTS_BYTES + STEP_BYTES * step_count;
        let mut end = table + OFFSET_BYTES * frame_count;
        let mut frames = Vec::with_capacity(frame_count);
        for index in 0..frame_count {
            let at = table + OFFSET_BYTES * index;
            let offset = usize::from(u16::from_le_bytes([byte(at)?, byte(at + 1)?]));
            let start = table + offset;
            let cells = usize::from(byte(start)?);
            let cells = (0..cells)
                .map(|cell| {
                    let at = start + 1 + CELL_BYTES * cell;
                    Ok(Cell {
                        size: byte(at)?,
                        x: byte(at + 1)? as i8,
                        y: byte(at + 2)? as i8,
                        u: byte(at + 3)?,
                        v: byte(at + 4)?,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            end = end.max(start + 1 + CELL_BYTES * cells.len());
            frames.push(Frame { cells });
        }

        let animation = Self { steps, frames };
        if animation.encode() != bytes[..end] {
            return Err(Error::AnimationStream(
                "an animation's frames are not laid out in table order after its offsets".into(),
            ));
        }
        Ok((animation, end))
    }

    /// The animation's bytes: the counts, the steps, the frame offsets and the frames in table order.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![self.frames.len() as u8, self.steps.len() as u8];
        for step in &self.steps {
            out.extend([step.delay, step.frame]);
        }
        let mut offset = OFFSET_BYTES * self.frames.len();
        for frame in &self.frames {
            out.extend((offset as u16).to_le_bytes());
            offset += 1 + CELL_BYTES * frame.cells.len();
        }
        for frame in &self.frames {
            out.push(frame.cells.len() as u8);
            for cell in &frame.cells {
                out.extend(cell.bytes());
            }
        }
        out
    }

    /// The steps that show a frame, with their positions in the stream: every step but the jumps.
    pub fn shown(&self) -> impl Iterator<Item = (usize, Step)> + '_ {
        self.steps.iter().copied().enumerate().filter(|(_, step)| step.jump().is_none())
    }

    /// The step a jump in the stream continues at, the animation's loop point.
    pub fn loop_to(&self) -> Option<usize> {
        self.steps.iter().find_map(|step| step.jump())
    }
}

/// The step function a handler calls once a tick.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Playback {
    /// `Actor_StepAnimLoop`: past the last step the stream starts again; a jump moves to its step.
    #[default]
    Loop,
    /// `Actor_StepAnimOnce`: past the last step the stream holds its frame; a jump moves to its step.
    Once,
    /// `Actor_StepAnimReverse`: the stream runs backwards and starts again from its end; a jump is not read.
    Reverse,
}

impl Playback {
    /// Every step function, in the order the views list them.
    pub const ALL: [Playback; 3] = [Playback::Loop, Playback::Once, Playback::Reverse];

    /// The name a view shows.
    pub fn label(self) -> &'static str {
        match self {
            Self::Loop => "Loop",
            Self::Once => "Once",
            Self::Reverse => "Reverse",
        }
    }
}

/// The stream state an actor keeps.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Player {
    /// The byte position in the step stream, two bytes a step.
    position: usize,
    /// Ticks until the next step, a byte that wraps as the game's does.
    delay: u8,
    /// The frame on screen.
    frame: usize,
    /// The step the shown frame came from.
    step: usize,
    /// Whether the last tick reported the end of the stream (the step function returned 1).
    ended: bool,
    /// Ticks since the start.
    ticks: u64,
}

impl Player {
    /// `Actor_SeekScript(0)`: the stream from its first step.
    pub fn start(animation: &Animation) -> Self {
        let mut player = Self { position: 0, delay: 0, frame: 0, step: 0, ended: false, ticks: 0 };
        player.stage(animation);
        player
    }

    /// The frame on screen.
    pub fn frame(&self) -> usize {
        self.frame
    }

    /// The step the frame on screen came from.
    pub fn step(&self) -> usize {
        self.step
    }

    /// Ticks until the stream moves on.
    pub fn delay(&self) -> u8 {
        self.delay
    }

    /// Whether the last tick reached the end of the stream.
    pub fn ended(&self) -> bool {
        self.ended
    }

    /// Ticks played.
    pub fn ticks(&self) -> u64 {
        self.ticks
    }

    /// One tick of `playback`: the step function a handler calls each frame.
    pub fn tick(&mut self, animation: &Animation, playback: Playback) {
        self.ticks += 1;
        self.ended = false;
        self.delay = self.delay.wrapping_sub(1);
        if self.delay != 0 {
            return;
        }

        let steps = animation.steps.len();
        match playback {
            Playback::Loop => {
                if self.position / STEP_BYTES == steps {
                    self.position = 0;
                    self.ended = true;
                } else if let Some(target) = self.jump(animation) {
                    self.position = target * STEP_BYTES;
                    self.ended = true;
                }
                self.stage(animation);
            }
            Playback::Once => {
                if self.position / STEP_BYTES == steps {
                    self.delay = 1;
                    self.ended = true;
                    return;
                }
                if let Some(target) = self.jump(animation) {
                    self.position = target * STEP_BYTES;
                    self.ended = true;
                }
                self.stage(animation);
            }
            Playback::Reverse => {
                if self.position == 0 {
                    self.position = steps * STEP_BYTES;
                    self.ended = true;
                }
                self.position = self.position.saturating_sub(STEP_BYTES);
                let step =
                    animation.steps.get(self.position / STEP_BYTES).copied().unwrap_or(Step { delay: 0, frame: 0 });
                self.delay = step.delay;
                self.frame = usize::from(step.frame);
                self.step = self.position / STEP_BYTES;
            }
        }
    }

    /// The jump at the current position, when the step there is one.
    fn jump(&self, animation: &Animation) -> Option<usize> {
        animation.steps.get(self.position / STEP_BYTES).and_then(|step| step.jump())
    }

    /// `Actor_StageAnimStep`: stages the step at the position and moves past it.
    fn stage(&mut self, animation: &Animation) {
        let index = self.position / STEP_BYTES;
        let step = animation.steps.get(index).copied().unwrap_or(Step { delay: 0, frame: 0 });
        self.delay = step.delay;
        self.frame = usize::from(step.frame);
        self.step = index;
        self.position += STEP_BYTES;
    }
}

fn short(at: usize) -> Error {
    Error::AnimationStream(format!("an animation ends before byte {at:#X}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(u: u8) -> Cell {
        Cell { size: 5, x: -8, y: -16, u, v: 0 }
    }

    /// Three frames: steps 0, 1, 2 with delays 2, 1, 3, then a jump back to step 1.
    fn walk() -> Animation {
        Animation {
            steps: vec![
                Step { delay: 2, frame: 0 },
                Step { delay: 1, frame: 1 },
                Step { delay: 3, frame: 2 },
                Step { delay: JUMP, frame: 1 },
            ],
            frames: vec![Frame { cells: vec![cell(0)] }, Frame { cells: vec![cell(16), cell(32)] }, Frame::default()],
        }
    }

    #[test]
    fn an_animation_decodes_to_what_it_encodes() {
        let animation = walk();
        let bytes = animation.encode();
        let (decoded, end) = Animation::decode(&bytes).expect("decodes");
        assert_eq!(decoded, animation);
        assert_eq!(end, bytes.len());
        assert_eq!(decoded.loop_to(), Some(1));
        assert_eq!(decoded.shown().count(), 3);
    }

    #[test]
    fn frames_out_of_table_order_are_refused() {
        let mut bytes = walk().encode();
        // Point frame 0 at frame 1's bytes.
        let table = COUNTS_BYTES + STEP_BYTES * 4;
        bytes[table] = bytes[table + OFFSET_BYTES];
        assert!(Animation::decode(&bytes).is_err());
    }

    /// The frames each tick shows, starting from `Player::start`.
    fn frames(playback: Playback, ticks: usize) -> Vec<usize> {
        let animation = walk();
        let mut player = Player::start(&animation);
        let mut shown = vec![player.frame()];
        for _ in 1..ticks {
            player.tick(&animation, playback);
            shown.push(player.frame());
        }
        shown
    }

    #[test]
    fn a_loop_follows_its_jump() {
        // Frame 0 for 2 ticks, 1 for 1, 2 for 3, then the jump stages step 1 again.
        assert_eq!(frames(Playback::Loop, 10), [0, 0, 1, 2, 2, 2, 1, 2, 2, 2]);
    }

    #[test]
    fn once_follows_its_jump_like_the_loop() {
        assert_eq!(frames(Playback::Once, 7), [0, 0, 1, 2, 2, 2, 1]);
    }

    #[test]
    fn once_holds_the_last_frame_of_a_stream_without_a_jump() {
        let mut animation = walk();
        animation.steps.pop();
        let mut player = Player::start(&animation);
        for _ in 0..20 {
            player.tick(&animation, Playback::Once);
        }
        assert_eq!(player.frame(), 2);
        assert!(player.ended());
    }

    #[test]
    fn reverse_runs_the_stream_backwards() {
        let mut animation = walk();
        animation.steps.pop();
        let mut player = Player::start(&animation);
        let mut shown = Vec::new();
        for _ in 0..8 {
            player.tick(&animation, Playback::Reverse);
            shown.push(player.frame());
        }
        assert_eq!(shown, [0, 0, 0, 2, 2, 2, 1, 0]);
    }
}
