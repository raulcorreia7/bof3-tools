//! A message laid out the way the message window prints it: pages, lines and colour runs.
//!
//! The layout reads the `{TAG}` text that `codec::decode` writes and follows `wiki: data/text`:
//! `{NEWLINE}` starts a line, `{PAGE}` waits for a button and starts a page, `{COLOR:nn}` and
//! `{NOCOLOR}` switch the glyph colour, and `{BACKSPACE}` steps back one glyph. Codes that read
//! another string (`{GOTO}`, `{INSERT}`, `{VAR}`, `{LINK}`) and `{NUM}` show as a placeholder,
//! because their text is only known while the game runs. Codes that only time or box the window
//! draw nothing.

use gpui::{Div, div, prelude::*, px};

use crate::theme::{ACCENT, BORDER, CORNER, MUTED, PANEL, TEXT, color};
use crate::widgets::{GAP, GAP_SMALL, mono};

/// The `{COLOR}` operands the wiki names.
const RED: u8 = 0x02;
const BLUE: u8 = 0x03;
/// The most common operand on the disc, drawn in the window's own colour. The wiki does not name it.
const PLAIN: u8 = 0x00;
/// Screen colours for the named operands.
const RED_TEXT: u32 = 0xE0_6C_5A;
const BLUE_TEXT: u32 = 0x6A_A8_F0;
/// `{COLOR}` reads only the operand's low four bits (`0x7800 | (n & 0xF)`).
const COLOR_MASK: u8 = 0x0F;
/// The width of the window box, in pixels.
const WINDOW_WIDTH: f32 = 360.0;
/// The height of a printed line, in pixels; the window steps 12 px per line in the game.
const LINE_HEIGHT: f32 = 16.0;

/// A run of glyphs in one colour. `color` is the `{COLOR}` operand, or `None` for the window's own
/// colour. A placeholder stands for text the game fills in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    pub color: Option<u8>,
    pub placeholder: bool,
}

pub type Line = Vec<Run>;
pub type Page = Vec<Line>;

/// One `{BODY}` or `{BODY:hex}` marker, or one glyph.
enum Piece<'a> {
    Glyph(char),
    Tag { body: &'a str, operand: Option<u8> },
}

/// The marker that opens `text`, and the bytes it takes.
fn tag(text: &str) -> Option<(Piece<'_>, usize)> {
    let close = text.find('}')?;
    let inner = &text[1..close];
    let (body, operand) = match inner.split_once(':') {
        Some((body, hex)) => (body, Some(u8::from_str_radix(hex, 16).ok()?)),
        None => (inner, None),
    };
    let named = body.starts_with(|c: char| c.is_ascii_uppercase())
        && body.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
    named.then_some((Piece::Tag { body, operand }, close + 1))
}

fn pieces(text: &str) -> Vec<Piece<'_>> {
    let mut pieces = Vec::new();
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if c == '{'
            && let Some((piece, taken)) = tag(rest)
        {
            pieces.push(piece);
            rest = &rest[taken..];
            continue;
        }
        pieces.push(Piece::Glyph(c));
        rest = &rest[c.len_utf8()..];
    }
    pieces
}

/// The text a reading code stands for.
fn placeholder(body: &str, operand: Option<u8>) -> Option<String> {
    let operand = operand.unwrap_or_default();
    match body {
        "GOTO" => Some("‹leader›".into()),
        "INSERT" => Some(format!("‹name {operand}›")),
        "VAR" => Some(format!("‹var {operand}›")),
        "LINK" => Some(format!("‹link {operand}›")),
        "NUM" => Some(format!("‹{operand}›")),
        "RAW" => Some(format!("‹{operand:02X}›")),
        _ => None,
    }
}

/// Builds pages, lines and runs as the window would print them.
struct Layout {
    pages: Vec<Page>,
    color: Option<u8>,
}

impl Layout {
    fn line(&mut self) -> &mut Line {
        let page = self.pages.last_mut().expect("a layout always has a page");
        page.last_mut().expect("a page always has a line")
    }

    fn push(&mut self, text: &str, placeholder: bool) {
        let color = self.color;
        let line = self.line();
        match line.last_mut() {
            Some(run) if run.color == color && run.placeholder == placeholder && !placeholder => {
                run.text.push_str(text)
            }
            _ => line.push(Run { text: text.into(), color, placeholder }),
        }
    }

    fn backspace(&mut self) {
        let line = self.line();
        if let Some(run) = line.last_mut() {
            run.text.pop();
            if run.text.is_empty() {
                line.pop();
            }
        }
    }
}

/// The pages of a decoded message. Trailing `{END}` padding draws nothing; an `{END}` with text after
/// it, as between `{CHOICE}` answers, starts a line.
pub fn pages(text: &str) -> Vec<Page> {
    let mut layout = Layout { pages: vec![vec![Vec::new()]], color: None };
    let pieces = pieces(text);
    let last_glyph = pieces.iter().rposition(|piece| !matches!(piece, Piece::Tag { body: "END", .. }));

    for (at, piece) in pieces.iter().enumerate() {
        match *piece {
            Piece::Glyph(glyph) => layout.push(glyph.encode_utf8(&mut [0; 4]), false),
            Piece::Tag { body: "NEWLINE", .. } => layout.pages.last_mut().expect("a page").push(Vec::new()),
            Piece::Tag { body: "END", .. } if last_glyph.is_some_and(|last| at < last) => {
                layout.pages.last_mut().expect("a page").push(Vec::new());
            }
            Piece::Tag { body: "PAGE", .. } => layout.pages.push(vec![Vec::new()]),
            Piece::Tag { body: "COLOR", operand } => layout.color = operand.map(|n| n & COLOR_MASK),
            Piece::Tag { body: "NOCOLOR", .. } => layout.color = None,
            Piece::Tag { body: "BACKSPACE", .. } => layout.backspace(),
            Piece::Tag { body, operand } => {
                if let Some(text) = placeholder(body, operand) {
                    layout.push(&text, true);
                }
            }
        }
    }

    layout.pages.retain(|page| page.iter().any(|line| !line.is_empty()));
    layout.pages
}

fn run_color(run: &Run) -> u32 {
    match run.color {
        None if run.placeholder => MUTED,
        None | Some(PLAIN) => TEXT,
        Some(RED) => RED_TEXT,
        Some(BLUE) => BLUE_TEXT,
        Some(_) => ACCENT,
    }
}

/// The message drawn as window boxes, one per page.
pub fn window(text: &str) -> Div {
    let mut boxes = div().flex().flex_wrap().gap(px(GAP_SMALL));
    for page in pages(text) {
        let mut lines = div()
            .flex()
            .flex_col()
            .w(px(WINDOW_WIDTH))
            .p(px(GAP))
            .rounded(px(CORNER))
            .bg(color(PANEL))
            .border_1()
            .border_color(color(BORDER));
        for line in page {
            let mut row = div().flex().min_h(px(LINE_HEIGHT)).whitespace_nowrap();
            for run in line {
                let color_value = run_color(&run);
                row = row.child(mono(run.text).text_color(color(color_value)));
            }
            lines = lines.child(row);
        }
        boxes = boxes.child(lines);
    }
    boxes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(text: &str) -> Run {
        Run { text: text.into(), color: None, placeholder: false }
    }

    #[test]
    fn newline_and_page_split_the_message() {
        let pages = pages("Hello{NEWLINE}there{PAGE}Bye{END}{END}");
        assert_eq!(pages, vec![vec![vec![plain("Hello")], vec![plain("there")]], vec![vec![plain("Bye")]]]);
    }

    #[test]
    fn color_runs_end_at_nocolor() {
        let pages = pages("a{COLOR:12}b{NOCOLOR}c");
        let runs = &pages[0][0];
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[1], Run { text: "b".into(), color: Some(RED), placeholder: false });
        assert_eq!(runs[2], plain("c"));
    }

    #[test]
    fn reading_codes_show_placeholders_and_window_codes_draw_nothing() {
        let pages = pages("{BOX:01}{SE:05}Hi {INSERT:01}{WAIT}!");
        let runs = &pages[0][0];
        assert_eq!(runs[0], plain("Hi "));
        assert_eq!(runs[1], Run { text: "‹name 1›".into(), color: None, placeholder: true });
        assert_eq!(runs[2], plain("!"));
    }

    #[test]
    fn an_end_between_answers_starts_a_line() {
        let pages = pages("{CHOICE:02}Yes{END}No{END}{END}");
        assert_eq!(pages[0], vec![vec![plain("Yes")], vec![plain("No")]]);
    }

    #[test]
    fn backspace_removes_a_glyph() {
        assert_eq!(pages("ab{BACKSPACE}c")[0][0], vec![plain("ac")]);
    }

    #[test]
    fn a_brace_that_is_not_a_tag_stays_a_glyph() {
        assert_eq!(pages("{x}")[0][0], vec![plain("{x}")]);
    }
}
