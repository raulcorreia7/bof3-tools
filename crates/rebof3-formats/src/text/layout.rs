//! A message as the message window lays it out: pages, glyph positions, colours, the runtime text a code
//! reads, the choice panel and the events the step machine runs.
//!
//! The layout follows the game's two passes over a message:
//!
//! - `UI_DrawMessageScript (exe/slus_004_22@0x80150470)` draws the page from its first byte every frame. It
//!   owns the glyph cursor, the colour, the redirects and the line breaks, and is the source of every position
//!   here.
//! - `func_8015083C (exe/slus_004_22@0x8015083C)` steps the message: it decides when a page ends, when the
//!   window waits and which codes only time or box the window. Its decompiled body is a restated draft
//!   (nonmatching), so the events here carry only what its case bodies show and the wiki states
//!   (`wiki: data/text#control-codes`).
//!
//! Nothing here is guessed: a code whose effect depends on state the data does not hold (a runtime name, a
//! shake offset) is reported as such, with the reason, and drawn at its resting place or not at all.

use std::collections::BTreeMap;

/// The control codes, by byte (`TextCommand`, `include/game/text.h`).
pub mod code {
    pub const END: u8 = 0x00;
    pub const NEWLINE: u8 = 0x01;
    pub const PAGE: u8 = 0x02;
    pub const GOTO: u8 = 0x03;
    pub const INSERT: u8 = 0x04;
    pub const COLOR: u8 = 0x05;
    pub const NOCOLOR: u8 = 0x06;
    pub const VAR: u8 = 0x07;
    pub const LINK: u8 = 0x08;
    pub const NOP: u8 = 0x09;
    pub const SE: u8 = 0x0A;
    pub const GAP: u8 = 0x0B;
    pub const BOX: u8 = 0x0C;
    pub const OFFSET: u8 = 0x0D;
    pub const CURSOR: u8 = 0x0E;
    pub const STYLE: u8 = 0x0F;
    pub const STYLE_ALT: u8 = 0x10;
    pub const WAIT: u8 = 0x11;
    pub const SKIP: u8 = 0x12;
    pub const SKIP_ALT: u8 = 0x13;
    pub const CHOICE: u8 = 0x14;
    pub const NUM: u8 = 0x15;
    pub const BOXEND: u8 = 0x16;
    pub const BACKSPACE: u8 = 0x90;
}

/// Pixels the glyph cursor advances per glyph: `cursor[0] += 8` in `UI_DrawMessageScript (exe/slus_004_22@0x80150470)`.
pub const GLYPH_ADVANCE: i32 = 8;
/// Pixels `{NEWLINE}` moves down in the message window: `cursor[1] += 0xE` in `UI_DrawMessageScript`.
pub const LINE_STEP: i32 = 14;
/// Pixels `{NEWLINE}` moves down in the choice panel: `y += 0xD` in `UI_DrawTextAt (exe/slus_004_22@0x8015A2A4)`.
pub const CHOICE_LINE_STEP: i32 = 13;
/// Where the text starts inside the window: `origin = panel + (10, 6)` in `UI_UpdateLayout (exe/slus_004_22@0x80150358)`.
pub const TEXT_ORIGIN: (i32, i32) = (10, 6);
/// The glyph sprite sits one row below its cursor: `D_80145ACC = y + 1` in `UI_DrawText (exe/slus_004_22@0x8014FEA4)`.
pub const GLYPH_DROP: i32 = 1;
/// Bytes a redirect reads before it gives up: `count` less one (`{GOTO}` and `{INSERT}` 6, `{VAR}` 33, `{LINK}` 17).
const GOTO_BUDGET: u8 = 6;
const VAR_BUDGET: u8 = 33;
const LINK_BUDGET: u8 = 17;
/// The bytes of one character name's field in the party record: the longest `{INSERT}` or `{GOTO}` text.
pub const NAME_FIELD_BYTES: usize = (GOTO_BUDGET - 1) as usize;
/// Bytes of the window-style flags: bit 7 hides the frame, bit 6 picks the narrow window
/// (`func_80159050 (exe/slus_004_22@0x80159050)`).
const BOX_NO_FRAME: u8 = 0x80;
const BOX_NARROW: u8 = 0x40;
/// Low nibble of a choice's `{BOX}` operand: how many answers it holds
/// (`s2[2] = (operand & 0xF) - 1` in `func_8015083C (exe/slus_004_22@0x8015083C)`).
const ANSWER_COUNT_MASK: u8 = 0x0F;
/// The choice panel: left edge, width and the text's left margin
/// (`func_8015A554 (exe/slus_004_22@0x8015A554)`: `unk_18 = 0x30`, `unk_0A = 0xDC`; `UI_DrawTextRows` passes x `0x3A`).
pub const CHOICE_PANEL_X: i32 = 0x30;
pub const CHOICE_PANEL_WIDTH: i32 = 0xDC;
const CHOICE_TEXT_X: i32 = 0x3A;
/// The choice panel's vertical centre, its padding and the gap between answers
/// (`func_8015A554`: `0x78 - (height + rows) / 2`, `height + 8 + rows * 2`, `base + line * 0xD + row * 2`).
const CHOICE_CENTRE_Y: i32 = 0x78;
const CHOICE_TEXT_DROP: i32 = 4;
const CHOICE_PADDING: i32 = 8;
const CHOICE_ROW_GAP: i32 = 2;

/// Which window frame the message shows in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowKind {
    /// 200 by 54 pixels (`unk_0A = 0xC8`, `unk_0B = 0x36` in `func_80159050`).
    Wide,
    /// 120 by 54 pixels (`unk_0A = 0x78`), picked by bit 6 of the leading `{BOX}` operand.
    Narrow,
}

impl WindowKind {
    /// The frame's width and height in pixels.
    pub fn size(self) -> (i32, i32) {
        match self {
            Self::Wide => (200, 54),
            Self::Narrow => (120, 54),
        }
    }
}

/// Where and how the window opens: the leading `{BOX}` operand, which `UI_ResetMessage
/// (exe/slus_004_22@0x80151BDC)` stores in `D_801490CE` and `func_80159050` reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Placement {
    /// The operand, or `None` when the message does not open with `{BOX}` (the game then uses 0).
    pub operand: Option<u8>,
    /// The row of the panel-origin table the window settles at (`g_MenuPanelOrigins`, `0x8017F90C`).
    pub index: u8,
    pub kind: WindowKind,
    /// Whether the frame is drawn: bit 7 of the operand hides it (`func_80159510` tests panel flag 1).
    pub framed: bool,
}

impl Placement {
    fn of(operand: Option<u8>) -> Self {
        let value = operand.unwrap_or(0);
        Self {
            operand,
            index: value & !(BOX_NO_FRAME | BOX_NARROW),
            kind: if value & BOX_NARROW != 0 { WindowKind::Narrow } else { WindowKind::Wide },
            framed: value & BOX_NO_FRAME == 0,
        }
    }
}

/// What a glyph draws.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Shape {
    /// The message font's cell for this byte (`UI_DrawTextGlyphs`, the default case).
    Byte(u8),
    /// The 21-column number sheet's cell for `{NUM}` operand `n`.
    Number(u8),
}

/// One glyph the page draws.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Glyph {
    /// Left edge, in pixels from the window's top-left.
    pub x: i32,
    /// Top edge of the sprite (the cursor's `y` plus [`GLYPH_DROP`]).
    pub y: i32,
    pub shape: Shape,
    /// The `{COLOR}` operand: the text CLUT cell is its low four bits. 0 after `{NOCOLOR}`.
    pub color: u8,
    /// Drawn between `{NOP0D}` and `{NOP0E}`: the game places it at the window state's animated position, which
    /// the data does not hold, so the layout draws it at its cursor.
    pub animated: bool,
}

/// A code that reads text the data does not hold, or text from another place.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Redirect {
    /// `{GOTO}`: the party leader's name.
    Leader,
    /// `{INSERT:n}`: the name field of party record `n`.
    Character(u8),
    /// `{VAR:n}`: the 32-byte string number `n` that `Message_SetVariable` fills.
    Variable(u8),
    /// `{LINK:n}`: a string of the bank's own sub-table.
    Link(u8),
}

impl Redirect {
    /// The most bytes the game reads from the other string.
    pub fn budget(self) -> usize {
        usize::from(match self {
            Self::Leader | Self::Character(_) => GOTO_BUDGET,
            Self::Variable(_) => VAR_BUDGET,
            Self::Link(_) => LINK_BUDGET,
        }) - 1
    }
}

/// A redirect that the page could not read, drawn as a placeholder as wide as the game could print.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Runtime {
    pub x: i32,
    pub y: i32,
    pub what: Redirect,
    /// Glyph cells the placeholder spans: the redirect's byte budget.
    pub cells: usize,
}

/// What the step machine does at a code that draws nothing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventKind {
    /// `{SE:n}`: queues cue `n | 0x200` (`SND_PlayCue (exe/slus_004_22@0x8015DF18)`).
    Sound(u8),
    /// `{GAP}`: waits eight more frames before the next glyph.
    Gap,
    /// `{WAIT}` and `{STYLE10}`: toggle the flag that prints without waiting for the glyph delay.
    ToggleInstant,
    /// `{STYLE0F:n}`: loads the style parameters of row `n` (`D_8017F5B4`).
    Style(u8),
    /// `{BOX:n}` after the first byte: skipped by both passes.
    Box(u8),
    /// `{BOXEND:n}`: stores the operand and closes the box.
    BoxEnd(u8),
    /// `{NOP}`: no effect.
    Nop,
    /// `{OFFSET}` and `{CURSOR}`: start and end the animated glyph run.
    Offset(bool),
}

/// An event, placed after `glyphs` glyphs of the page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Event {
    pub glyphs: usize,
    pub kind: EventKind,
}

/// What the layout could not settle, with the reason.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Unproven {
    /// The byte offset in the message.
    pub offset: usize,
    pub why: String,
}

/// One answer of a choice, laid out in the choice panel.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Answer {
    pub glyphs: Vec<Glyph>,
    pub runtime: Vec<Runtime>,
    pub lines: usize,
}

/// The choice a page ends with.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Choice {
    /// The `{CHOICE}` operand: the answer-message selector the game stores.
    pub selector: u8,
    /// The answer count the following `{BOX}` operand names, when the message has one.
    pub declared: Option<u8>,
    /// The `{BOX}` operand's high nibble: zero picks step state 5, anything else state 4.
    pub mode: u8,
    pub answers: Vec<Answer>,
}

impl Choice {
    /// The panel's frame in screen pixels, from `func_8015A554`: left, top, width and height.
    pub fn panel(&self) -> (i32, i32, i32, i32) {
        let lines: i32 = self.answers.iter().map(|answer| answer.lines as i32).sum();
        let last_row = self.answers.len().saturating_sub(1) as i32;
        let height = lines * CHOICE_LINE_STEP;
        let top = CHOICE_CENTRE_Y - (height + last_row) / 2;
        (CHOICE_PANEL_X, top, CHOICE_PANEL_WIDTH, height + CHOICE_PADDING + last_row * CHOICE_ROW_GAP)
    }

    /// The text origin of answer `row` in screen pixels.
    pub fn answer_origin(&self, row: usize) -> (i32, i32) {
        let before: i32 = self.answers[..row].iter().map(|answer| answer.lines as i32).sum();
        let (_, top, _, _) = self.panel();
        (CHOICE_TEXT_X, top + CHOICE_TEXT_DROP + before * CHOICE_LINE_STEP + row as i32 * CHOICE_ROW_GAP)
    }
}

/// How a page ends.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PageEnd {
    /// `{PAGE}`: the window waits for a button, then draws the next page.
    Wait,
    /// The message's `{END}`.
    End,
    Choice(Choice),
}

/// One page: what the window shows between two `{PAGE}` codes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Page {
    pub glyphs: Vec<Glyph>,
    pub runtime: Vec<Runtime>,
    pub events: Vec<Event>,
    /// Lines the page draws.
    pub lines: usize,
    pub end: PageEnd,
}

impl Page {
    /// The right edge of line `line`, in pixels from the window's left: the cursor's last position, counting each
    /// runtime placeholder at its full budget.
    pub fn line_widths(&self) -> Vec<i32> {
        let mut widths = vec![0; self.lines.max(1)];
        let row = |y: i32| ((y - GLYPH_DROP - TEXT_ORIGIN.1) / LINE_STEP).max(0) as usize;
        for glyph in &self.glyphs {
            let line = row(glyph.y).min(widths.len() - 1);
            widths[line] = widths[line].max(glyph.x + GLYPH_ADVANCE);
        }
        for runtime in &self.runtime {
            let line = row(runtime.y).min(widths.len() - 1);
            widths[line] = widths[line].max(runtime.x + runtime.cells as i32 * GLYPH_ADVANCE);
        }
        widths
    }
}

/// The laid-out message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Layout {
    pub placement: Placement,
    pub pages: Vec<Page>,
    pub unproven: Vec<Unproven>,
}

/// What a layout reads besides the message: the bank, for `{LINK}`, and the text a runtime code stands for.
#[derive(Clone, Copy, Default)]
pub struct Context<'a> {
    /// The bank the message belongs to.
    pub bank: Option<&'a [u8]>,
    /// The bytes a redirect reads, when the caller knows them (a default party name, say).
    pub runtime: Option<&'a dyn Fn(Redirect) -> Option<Vec<u8>>>,
}

/// Lays `message` out. `message` is the stored bytes, terminator and padding included.
pub fn layout(message: &[u8], context: Context<'_>) -> Layout {
    let placement = Placement::of((message.first() == Some(&code::BOX)).then(|| message.get(1).copied()).flatten());
    let mut machine = Machine::new(message, context, TEXT_ORIGIN, LINE_STEP, true);
    machine.run();
    let mut pages = machine.pages;
    // A trailing page with nothing on it is the padding after `{END}`, not a page.
    while pages.len() > 1 && pages.last().is_some_and(is_blank) {
        pages.pop();
    }
    Layout { placement, pages, unproven: machine.unproven }
}

fn is_blank(page: &Page) -> bool {
    page.glyphs.is_empty() && page.runtime.is_empty() && page.events.is_empty() && matches!(page.end, PageEnd::End)
}

/// A position in one of the byte streams the machine reads: the message, the bank, or a redirected string.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Cursor {
    stream: usize,
    at: usize,
}

/// The redirect state: `flags & 1`, `saved` and `count` of `UI_DrawMessageScript`.
struct Redirected {
    saved: Cursor,
    count: u8,
}

struct Machine<'a> {
    streams: Vec<Vec<u8>>,
    context: Context<'a>,
    origin: (i32, i32),
    line_step: i32,
    paged: bool,
    pages: Vec<Page>,
    unproven: Vec<Unproven>,
    x: i32,
    y: i32,
    color: u8,
    animated: bool,
    redirected: Option<Redirected>,
    /// Message offsets of the bytes read, for the report. Redirected bytes carry the redirect's offset.
    offsets: BTreeMap<usize, usize>,
}

impl<'a> Machine<'a> {
    fn new(bytes: &[u8], context: Context<'a>, origin: (i32, i32), line_step: i32, paged: bool) -> Self {
        let mut machine = Self {
            streams: vec![bytes.to_vec()],
            context,
            origin,
            line_step,
            paged,
            pages: Vec::new(),
            unproven: Vec::new(),
            x: origin.0,
            y: origin.1,
            color: 0,
            animated: false,
            redirected: None,
            offsets: BTreeMap::new(),
        };
        machine.start_page();
        machine
    }

    /// Resets what `UI_DrawMessageScript` resets at the top of every pass: the cursor and the colour.
    fn start_page(&mut self) {
        self.pages.push(Page {
            glyphs: Vec::new(),
            runtime: Vec::new(),
            events: Vec::new(),
            lines: 1,
            end: PageEnd::End,
        });
        (self.x, self.y) = self.origin;
        self.color = 0;
        self.animated = false;
        self.redirected = None;
    }

    fn page(&mut self) -> &mut Page {
        self.pages.last_mut().expect("a machine always has a page")
    }

    fn byte(&self, at: Cursor) -> Option<u8> {
        self.streams.get(at.stream).and_then(|stream| stream.get(at.at)).copied()
    }

    fn event(&mut self, kind: EventKind) {
        let glyphs = self.page().glyphs.len();
        self.page().events.push(Event { glyphs, kind });
    }

    fn unproven(&mut self, at: Cursor, why: impl Into<String>) {
        let offset = self.offsets.get(&at.stream).copied().unwrap_or(at.at);
        self.unproven.push(Unproven { offset, why: why.into() });
    }

    fn run(&mut self) {
        let mut at = Cursor { stream: 0, at: 0 };
        // The leading `{BOX}` is consumed by `UI_ResetMessage` for the placement; both passes then skip it.
        loop {
            if let Some(redirect) = &mut self.redirected {
                redirect.count -= 1;
                if redirect.count == 0 {
                    at = Cursor { at: redirect.saved.at + 1, ..redirect.saved };
                    self.redirected = None;
                }
            }
            let Some(op) = self.byte(at) else { return };
            match op {
                code::END => {
                    if let Some(redirect) = self.redirected.take() {
                        at = redirect.saved;
                    } else {
                        return;
                    }
                }
                code::SE | code::BOX | code::STYLE | code::CHOICE => {
                    let operand = self.byte(Cursor { at: at.at + 1, ..at }).unwrap_or(0);
                    if op == code::CHOICE {
                        self.choice(at, operand);
                        return;
                    }
                    let kind = match op {
                        code::SE => EventKind::Sound(operand),
                        code::STYLE => EventKind::Style(operand),
                        _ => EventKind::Box(operand),
                    };
                    self.event(kind);
                    at.at += 1;
                }
                code::NEWLINE => {
                    self.x = self.origin.0;
                    self.y += self.line_step;
                    self.page().lines += 1;
                }
                code::PAGE if self.paged => {
                    self.page().end = PageEnd::Wait;
                    self.start_page();
                }
                code::VAR | code::INSERT | code::LINK | code::GOTO => {
                    at = self.redirect(at, op);
                }
                code::COLOR => {
                    at.at += 1;
                    self.color = self.byte(at).unwrap_or(0);
                }
                code::NOCOLOR => self.color = 0,
                code::PAGE => {}
                code::NOP => self.event(EventKind::Nop),
                code::GAP => self.event(EventKind::Gap),
                code::STYLE_ALT | code::WAIT => self.event(EventKind::ToggleInstant),
                code::BOXEND => {
                    // The step machine stops at `{BOXEND}` and reads its operand; the draw pass never reaches it.
                    let operand = self.byte(Cursor { at: at.at + 1, ..at }).unwrap_or(0);
                    self.event(EventKind::BoxEnd(operand));
                    at.at += 1;
                }
                code::OFFSET => {
                    self.animated = true;
                    self.event(EventKind::Offset(true));
                }
                code::CURSOR => {
                    self.animated = false;
                    self.event(EventKind::Offset(false));
                }
                _ => at = self.glyph(at, op),
            }
            at.at = at.at.wrapping_add(1);
        }
    }

    /// The default case of `UI_DrawMessageScript`: draws one glyph at the cursor and advances it.
    fn glyph(&mut self, mut at: Cursor, op: u8) -> Cursor {
        if op == code::BACKSPACE && self.x == self.origin.0 {
            self.x -= GLYPH_ADVANCE;
        }
        let shape = match op {
            code::NUM => {
                at.at += 1;
                Shape::Number(self.byte(at).unwrap_or(0))
            }
            code::SKIP | code::SKIP_ALT => {
                at.at += 1;
                Shape::Byte(op)
            }
            _ => Shape::Byte(op),
        };
        let glyph = Glyph { x: self.x, y: self.y + GLYPH_DROP, shape, color: self.color, animated: self.animated };
        self.page().glyphs.push(glyph);
        self.x += GLYPH_ADVANCE;
        at
    }

    /// `{GOTO}`, `{INSERT}`, `{VAR}` and `{LINK}`: reads another string for at most the redirect's budget.
    fn redirect(&mut self, at: Cursor, op: u8) -> Cursor {
        let mut at = at;
        let (what, budget) = match op {
            code::GOTO => (Redirect::Leader, GOTO_BUDGET),
            code::INSERT => {
                at.at += 1;
                (Redirect::Character(self.byte(at).unwrap_or(0)), GOTO_BUDGET)
            }
            code::VAR => {
                at.at += 1;
                (Redirect::Variable(self.byte(at).unwrap_or(0)), VAR_BUDGET)
            }
            _ => {
                at.at += 1;
                (Redirect::Link(self.byte(at).unwrap_or(0)), LINK_BUDGET)
            }
        };
        let bytes = self.resolve(what);
        match bytes {
            Some(bytes) => {
                self.streams.push(bytes);
                self.redirected = Some(Redirected { saved: at, count: budget });
                // The next loop reads the redirected string from its first byte; the shared tail steps over
                // the byte before it (`p = base; ... p++`), which the stream here does not hold.
                self.pending_jump(self.streams.len() - 1)
            }
            None => {
                let cells = what.budget();
                let (x, y) = (self.x, self.y + GLYPH_DROP);
                self.page().runtime.push(Runtime { x, y, what, cells });
                self.x += cells as i32 * GLYPH_ADVANCE;
                self.unproven(at, format!("{what:?} reads text that only exists while the game runs; the layout reserves its widest {cells} glyphs"));
                at
            }
        }
    }

    /// The cursor one step before the start of stream `stream`, so the shared `p++` lands on its first byte.
    fn pending_jump(&self, stream: usize) -> Cursor {
        Cursor { stream, at: usize::MAX }
    }

    fn resolve(&self, what: Redirect) -> Option<Vec<u8>> {
        if let Redirect::Link(operand) = what
            && let Some(bank) = self.context.bank
        {
            return link_bytes(bank, operand);
        }
        self.context.runtime.and_then(|resolve| resolve(what))
    }

    /// `{CHOICE}`: the page ends, and the answers follow the `{BOX}` that names their count.
    fn choice(&mut self, at: Cursor, selector: u8) {
        let after = Cursor { at: at.at + 2, ..at };
        let (declared, rest_from) = match (self.byte(after), self.byte(Cursor { at: after.at + 1, ..after })) {
            (Some(code::BOX), Some(operand)) => (Some(operand), after.at + 2),
            _ => (None, after.at),
        };
        let mode = declared.map_or(0, |operand| operand >> 4);
        let count = declared.map(|operand| usize::from(operand & ANSWER_COUNT_MASK));
        let stream = self.streams[at.stream].clone();
        let mut answers = Vec::new();
        let mut from = rest_from;
        while from < stream.len() && count.is_none_or(|count| answers.len() < count) {
            let end = stream[from..].iter().position(|&byte| byte == code::END).map_or(stream.len(), |end| from + end);
            if end == from && count.is_none() {
                break;
            }
            answers.push(self.answer(&stream[from..end]));
            from = end + 1;
        }
        if let Some(count) = count
            && count != answers.len()
        {
            self.unproven(at, format!("the choice names {count} answers and the message holds {}", answers.len()));
        }
        self.page().end = PageEnd::Choice(Choice { selector, declared, mode, answers });
    }

    fn answer(&self, bytes: &[u8]) -> Answer {
        let mut machine = Machine::new(bytes, self.context, (0, 0), CHOICE_LINE_STEP, false);
        machine.run();
        let page = machine.pages.remove(0);
        Answer { glyphs: page.glyphs, runtime: page.runtime, lines: page.lines }
    }
}

/// The bytes `{LINK:n}` reads from `bank`: `D_80010004` holds the offset of a table of `u16` offsets, entry `n`
/// of which, added to the table's address, is one byte before the string (`UI_DrawMessageScript`, case
/// `STRING_LINK`; the shared tail's `p++` lands on the string). `None` when any of it lies outside the bank.
fn link_bytes(bank: &[u8], operand: u8) -> Option<Vec<u8>> {
    let table = u32::from_le_bytes(bank.get(4..8)?.try_into().ok()?) as usize;
    let entry = table.checked_add(usize::from(operand) * 2)?;
    let offset = usize::from(u16::from_le_bytes(bank.get(entry..entry + 2)?.try_into().ok()?));
    let start = table.checked_add(offset)?.checked_add(1)?;
    bank.get(start..).filter(|rest| !rest.is_empty()).map(<[u8]>::to_vec)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lay(bytes: &[u8]) -> Layout {
        layout(bytes, Context::default())
    }

    fn at(glyph: &Glyph) -> (i32, i32) {
        (glyph.x, glyph.y)
    }

    #[test]
    fn glyphs_advance_eight_pixels_and_newline_fourteen_rows() {
        let page = &lay(&[b'A', b'B', code::NEWLINE, b'C', code::END]).pages[0];
        assert_eq!(page.glyphs.iter().map(at).collect::<Vec<_>>(), [(10, 7), (18, 7), (10, 21)]);
        assert_eq!(page.lines, 2);
    }

    #[test]
    fn a_page_code_starts_a_page_and_resets_the_colour() {
        let layout = lay(&[code::COLOR, 2, b'A', code::PAGE, b'B', code::END]);
        assert_eq!(layout.pages.len(), 2);
        assert_eq!(layout.pages[0].end, PageEnd::Wait);
        assert_eq!(layout.pages[0].glyphs[0].color, 2);
        assert_eq!(layout.pages[1].glyphs[0].color, 0);
    }

    #[test]
    fn backspace_at_the_line_start_steps_back_one_glyph() {
        let page = &lay(&[code::BACKSPACE, b'A', code::END]).pages[0];
        assert_eq!(page.glyphs[0].x, TEXT_ORIGIN.0 - GLYPH_ADVANCE);
        assert_eq!(page.glyphs[1].x, TEXT_ORIGIN.0);
    }

    #[test]
    fn the_leading_box_names_the_window() {
        let narrow = lay(&[code::BOX, 0x41, b'A', code::END]);
        assert_eq!(narrow.placement.kind, WindowKind::Narrow);
        assert_eq!(narrow.placement.index, 1);
        assert!(narrow.placement.framed);
        assert!(!lay(&[code::BOX, 0x81, b'A', code::END]).placement.framed);
        assert_eq!(lay(&[b'A', code::END]).placement.operand, None);
    }

    #[test]
    fn a_runtime_name_reserves_its_budget_and_is_reported() {
        let layout = lay(&[code::INSERT, 2, b'!', code::END]);
        let page = &layout.pages[0];
        assert_eq!(page.runtime.len(), 1);
        assert_eq!(page.runtime[0].cells, NAME_FIELD_BYTES);
        assert_eq!(page.glyphs[0].x, TEXT_ORIGIN.0 + NAME_FIELD_BYTES as i32 * GLYPH_ADVANCE);
        assert_eq!(layout.unproven.len(), 1);
    }

    #[test]
    fn a_known_name_is_drawn_through_the_redirect_budget() {
        let name = |what: Redirect| (what == Redirect::Character(2)).then(|| vec![b'R', b'y', b'u', code::END]);
        let context = Context { bank: None, runtime: Some(&name) };
        let layout = layout(&[code::INSERT, 2, b'!', code::END], context);
        let xs: Vec<i32> = layout.pages[0].glyphs.iter().map(|glyph| glyph.x).collect();
        assert_eq!(xs, [10, 18, 26, 34]);
        assert!(layout.unproven.is_empty());
    }

    #[test]
    fn a_redirect_stops_at_its_budget() {
        let long = |_: Redirect| Some(b"ABCDEFGH".to_vec());
        let context = Context { bank: None, runtime: Some(&long) };
        let layout = layout(&[code::INSERT, 0, b'!', code::END], context);
        // Five bytes of the name, then the glyph after the code.
        assert_eq!(layout.pages[0].glyphs.len(), NAME_FIELD_BYTES + 1);
    }

    #[test]
    fn a_choice_ends_the_page_with_its_answers() {
        let mut bytes = b"Q".to_vec();
        bytes.extend([code::CHOICE, 0, code::BOX, 2]);
        bytes.extend(b"Yes");
        bytes.push(code::END);
        bytes.extend(b"No");
        bytes.push(code::END);
        let layout = lay(&bytes);
        let PageEnd::Choice(choice) = &layout.pages[0].end else { panic!("no choice") };
        assert_eq!((choice.declared, choice.answers.len()), (Some(2), 2));
        assert_eq!(choice.answers[0].glyphs.len(), 3);
        assert_eq!(layout.pages[0].glyphs.len(), 1);
        let (left, top, width, height) = choice.panel();
        assert_eq!((left, width), (CHOICE_PANEL_X, CHOICE_PANEL_WIDTH));
        assert_eq!(height, 2 * CHOICE_LINE_STEP + CHOICE_PADDING + CHOICE_ROW_GAP);
        assert_eq!(top, CHOICE_CENTRE_Y - (2 * CHOICE_LINE_STEP + 1) / 2);
    }

    #[test]
    fn number_and_skip_codes_take_an_operand_and_draw_one_glyph() {
        let page = &lay(&[code::NUM, 3, code::SKIP, 9, b'A', code::END]).pages[0];
        assert_eq!(page.glyphs.len(), 3);
        assert_eq!(page.glyphs[0].shape, Shape::Number(3));
        assert_eq!(page.glyphs[1].shape, Shape::Byte(code::SKIP));
        assert_eq!(page.glyphs[2].x, TEXT_ORIGIN.0 + 2 * GLYPH_ADVANCE);
    }

    #[test]
    fn the_style_code_takes_the_next_byte_as_its_operand() {
        let page = &lay(&[code::STYLE, code::END, b'A', code::END]).pages[0];
        assert_eq!(page.events[0].kind, EventKind::Style(code::END));
        assert_eq!(page.glyphs.len(), 1);
    }

    #[test]
    fn a_link_outside_the_bank_is_a_placeholder() {
        let layout = layout(&[code::LINK, 7, code::END], Context { bank: Some(&[0u8; 8]), runtime: None });
        assert_eq!(layout.pages[0].runtime.len(), 1);
    }
}
