//! A hex editor with a value inspector: the bytes at the cursor read as every integer width.

use gpui::{
    AnyElement, Context, Keystroke, ScrollStrategy, UniformListScrollHandle, div, prelude::*, px, uniform_list,
};

use super::{Entry, Field, input};
use crate::app::Editor;
use crate::fonts;
use crate::theme::{EDITED, MUTED, ON_SELECTION, SELECTION, color};
use crate::widgets::{GAP, GAP_SECTION, GAP_SMALL, ROW_HEIGHT, mono, muted, strong};

/// Bytes shown on one row.
const ROW_BYTES: usize = 16;
/// Hex digits in a byte.
const BYTE_DIGITS: usize = 2;
/// Bits in a hex digit.
const DIGIT_BITS: u32 = 4;
/// Hex digits in a row offset.
const OFFSET_DIGITS: usize = 8;
/// Space between two bytes of a row, in pixels.
const BYTE_GAP: f32 = 4.0;
/// The value of 1.0 in 16.16 fixed point.
const FIXED_ONE: f64 = 65_536.0;
/// Width of an inspector field and of its label, in pixels.
const FIELD_WIDTH: f32 = 120.0;
const LABEL_WIDTH: f32 = 90.0;
/// Width of the go-to field, in pixels.
const GOTO_WIDTH: f32 = 80.0;
/// Width of the inspector column, in pixels.
const INSPECTOR_WIDTH: f32 = 240.0;

/// The integer widths the inspector reads: label, bytes and signedness.
const WIDTHS: [(&str, usize, bool); 6] =
    [("u8", 1, false), ("s8", 1, true), ("u16", 2, false), ("s16", 2, true), ("u32", 4, false), ("s32", 4, true)];

#[derive(Default)]
pub struct State {
    cursor: usize,
    /// The first hex digit typed into the cursor byte, waiting for the second.
    pending: Option<u8>,
    pub goto: String,
    scroll: UniformListScrollHandle,
}

impl State {
    fn move_to(&mut self, at: usize) {
        self.cursor = at;
        self.pending = None;
        self.scroll.scroll_to_item(at / ROW_BYTES, ScrollStrategy::Top);
    }
}

pub fn show(editor: &Editor, entry: &Entry<'_>, cx: &mut Context<Editor>) -> AnyElement {
    let length = entry.payload().len();
    if length == 0 {
        return muted("The entry is empty.").into_any_element();
    }
    let state = &editor.views.hex;
    let cursor = state.cursor.min(length - 1);

    let goto = input(editor, "goto", Field::Goto, &state.goto, "0x0", Some(GOTO_WIDTH), cx);
    let header = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(GAP))
        .child("Go to offset")
        .child(goto)
        .child(muted("Type hex digits to overwrite the byte at the cursor; arrows move."));

    let rows = length.div_ceil(ROW_BYTES);
    let grid = uniform_list(
        "hex",
        rows,
        cx.processor(|editor, range: std::ops::Range<usize>, _, cx| {
            let Some(entry) = editor.current() else { return Vec::new() };
            range.map(|row| grid_row(editor, &entry, row, cx)).collect()
        }),
    )
    .track_scroll(state.scroll.clone())
    .flex_1()
    .font_family(fonts::mono());

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .gap(px(GAP))
        .child(header)
        .child(div().flex().flex_1().min_h_0().gap(px(GAP_SECTION)).child(grid).child(inspector(
            editor,
            entry.payload(),
            cursor,
            cx,
        )))
        .into_any_element()
}

fn grid_row(editor: &Editor, entry: &Entry<'_>, row: usize, cx: &mut Context<Editor>) -> gpui::Div {
    let state = &editor.views.hex;
    let payload = entry.payload();
    let original = entry.original();
    let start = row * ROW_BYTES;
    let end = (start + ROW_BYTES).min(payload.len());

    let mut line = div()
        .flex()
        .h(px(ROW_HEIGHT))
        .items_center()
        .gap(px(BYTE_GAP))
        .child(div().text_color(color(MUTED)).mr(px(GAP)).child(format!("{start:0OFFSET_DIGITS$X}")));

    for (at, &byte) in payload.iter().enumerate().take(end).skip(start) {
        let text = match state.pending {
            Some(high) if at == state.cursor => format!("{high:X}_"),
            _ => format!("{byte:0BYTE_DIGITS$X}"),
        };
        let edited = original.get(at) != Some(&byte);
        let cell = div()
            .id(("byte", at))
            .px(px(1.0))
            .cursor_pointer()
            .when(edited, |cell| cell.text_color(color(EDITED)))
            .when(at == state.cursor, |cell| cell.bg(color(SELECTION)).text_color(color(ON_SELECTION)))
            .child(text)
            .on_click(cx.listener(move |editor, _, _, cx| {
                editor.views.hex.cursor = at;
                editor.views.hex.pending = None;
                cx.notify();
            }));
        line = line.child(cell);
    }

    let ascii: String =
        payload[start..end].iter().map(|&byte| if byte.is_ascii_graphic() { byte as char } else { '.' }).collect();
    line.child(div().ml(px(GAP)).text_color(color(MUTED)).child(ascii))
}

/// The integer widths at the cursor, each editable.
fn inspector(editor: &Editor, payload: &[u8], cursor: usize, cx: &mut Context<Editor>) -> gpui::Div {
    let mut column = div().flex().flex_col().flex_none().w(px(INSPECTOR_WIDTH)).gap(px(GAP_SMALL));
    column = column.child(strong(format!("Cursor {cursor:#X}")));

    for (row, &(label, width, signed)) in WIDTHS.iter().enumerate() {
        let Some(value) = read(payload, cursor, width, signed) else { continue };
        let value = input(editor, ("inspector", row), Field::Inspector(row), &value, "", Some(FIELD_WIDTH), cx)
            .font_family(fonts::mono());
        column = column.child(div().flex().items_center().child(div().w(px(LABEL_WIDTH)).child(label)).child(value));
    }

    if let Some(bytes) = payload.get(cursor..cursor + 4) {
        let word = u32::from_le_bytes(bytes.try_into().unwrap_or_default());
        let fixed = f64::from(word as i32) / FIXED_ONE;
        column = column
            .child(div().flex().child(div().w(px(LABEL_WIDTH)).child("16.16 fixed")).child(mono(format!("{fixed:.4}"))))
            .child(div().flex().child(div().w(px(LABEL_WIDTH)).child("pointer")).child(mono(format!("{word:#010X}"))));
    }
    column
}

/// The `width`-byte integer at `cursor`, as text.
fn read(payload: &[u8], cursor: usize, width: usize, signed: bool) -> Option<String> {
    let bytes = payload.get(cursor..cursor + width)?;
    let mut raw = [0u8; 8];
    raw[..width].copy_from_slice(bytes);
    let unsigned = u64::from_le_bytes(raw);
    let unused = u64::BITS - width as u32 * u8::BITS;
    Some(if signed { (((unsigned << unused) as i64) >> unused).to_string() } else { unsigned.to_string() })
}

/// Writes an inspector row's new text at the cursor.
pub fn inspector_changed(editor: &mut Editor, row: usize, text: &str) -> Option<String> {
    let (label, width, signed) = WIDTHS[row];
    let cursor = editor.views.hex.cursor;
    let mut payload = editor.current()?.payload().to_vec();
    payload.get(cursor..cursor + width)?;

    let text = text.trim();
    let parsed = if signed { text.parse::<i64>().ok().map(|value| value as u64) } else { text.parse::<u64>().ok() };
    let Some(number) = parsed else { return Some(format!("{label}: not a number")) };

    let stored = number.to_le_bytes();
    payload[cursor..cursor + width].copy_from_slice(&stored[..width]);
    editor.set_payload(payload);
    None
}

/// Moves the cursor to the offset the go-to field names.
pub fn go_to(editor: &mut Editor, text: &str) -> Option<String> {
    let length = editor.current()?.payload().len();
    match parse_offset(text) {
        Some(offset) if offset < length => {
            editor.views.hex.move_to(offset);
            None
        }
        _ => Some(format!("Offset must be below {length:#X}")),
    }
}

fn parse_offset(text: &str) -> Option<usize> {
    let text = text.trim();
    match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => usize::from_str_radix(hex, 16).ok(),
        None => text.parse().ok(),
    }
}

/// Arrows move the cursor; hex digits overwrite the byte at the cursor, high digit first.
pub fn key(editor: &mut Editor, keystroke: &Keystroke) -> bool {
    let Some(entry) = editor.current() else { return false };
    let mut payload = entry.payload().to_vec();
    let Some(last) = payload.len().checked_sub(1) else { return false };
    let state = &mut editor.views.hex;
    let cursor = state.cursor.min(last);

    let step = match keystroke.key.as_str() {
        "right" => 1isize,
        "left" => -1,
        "down" => ROW_BYTES as isize,
        "up" => -(ROW_BYTES as isize),
        _ => 0,
    };
    if step != 0 {
        state.move_to(cursor.saturating_add_signed(step).min(last));
        return true;
    }

    let Some(digit) = keystroke.key_char.as_deref().and_then(|typed| typed.chars().next()?.to_digit(16)) else {
        return false;
    };
    let digit = digit as u8;
    match state.pending.take() {
        None => state.pending = Some(digit),
        Some(high) => {
            payload[cursor] = high << DIGIT_BITS | digit;
            state.move_to((cursor + 1).min(last));
            editor.set_payload(payload);
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_widths_extend_the_sign() {
        assert_eq!(read(&[0xFF, 0x7F], 0, 1, true).as_deref(), Some("-1"));
        assert_eq!(read(&[0xFF, 0x7F], 0, 2, true).as_deref(), Some("32767"));
        assert_eq!(read(&[0xFF, 0xFF], 0, 2, false).as_deref(), Some("65535"));
        assert_eq!(read(&[0xFF], 0, 2, false), None);
    }

    #[test]
    fn offsets_read_as_hex_or_decimal() {
        assert_eq!(parse_offset("0x1F"), Some(31));
        assert_eq!(parse_offset(" 16 "), Some(16));
        assert_eq!(parse_offset("x"), None);
    }
}
