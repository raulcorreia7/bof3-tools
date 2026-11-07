//! The strings of an entry: a message bank's records, or the strings a scan finds in any other
//! payload.
//!
//! A string is rewritten in place. New text must encode to no more bytes than the old string took;
//! the rest of its span is filled with terminators, so no offset table or pointer moves.

use gpui::{AnyElement, Context, div, prelude::*, px};
use rebof3_emi::text::codec::{self, Encoding};
use rebof3_emi::text::discover::{self, Found, Kind};

use super::{Entry, Field, editing};
use crate::app::{Editor, Field as WindowField};
use crate::theme::{EDITED, MUTED, color};
use crate::widgets::{GAP, GAP_SMALL, field, mono, muted, separator, strong};

/// The byte that ends a string and pads a shortened one.
const TERMINATOR: u8 = 0;
/// Hex digits in a string's offset.
const OFFSET_DIGITS: usize = 6;
/// Width of the filter field and of the offset column, in pixels.
const FILTER_WIDTH: f32 = 200.0;
const OFFSET_WIDTH: f32 = 64.0;

#[derive(Default)]
pub struct State {
    pub filter: String,
}

fn found(payload: &[u8], load_arg: u32) -> Vec<Found> {
    discover::bank_records(payload).unwrap_or_else(|| discover::strings(payload, Some(load_arg)))
}

pub fn has_text(payload: &[u8], load_arg: u32) -> bool {
    !found(payload, load_arg).is_empty()
}

fn encoding(kind: Kind) -> Encoding {
    match kind {
        Kind::Field { .. } => Encoding::Name,
        Kind::Message | Kind::Record => Encoding::Message,
    }
}

pub fn show(editor: &Editor, entry: &Entry<'_>, cx: &mut Context<Editor>) -> AnyElement {
    let payload = entry.payload();
    let original = entry.original();
    let strings = found(payload, entry.load_arg());
    let state = &editor.views.text;

    let filter_text = state.filter.clone();
    let filter = field("text-filter", &state.filter, "Filter", editing(editor, Field::TextFilter), Some(FILTER_WIDTH))
        .on_click(cx.listener(move |editor, _, _, cx| {
            editor.start_editing(WindowField::View(Field::TextFilter), filter_text.clone());
            cx.notify();
        }));
    let header = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(GAP))
        .child(strong(format!("{} strings", strings.len())))
        .child(filter)
        .child(muted("New text must fit the old string's bytes. Shift+Enter breaks a line."));

    let needle = state.filter.to_lowercase();
    let mut list = div().id("strings").flex().flex_col().flex_1().gap(px(GAP_SMALL)).overflow_y_scroll();
    for string in &strings {
        let span = string.start..string.end;
        let text = codec::decode(encoding(string.kind), &payload[span.clone()]);
        if !needle.is_empty() && !text.to_lowercase().contains(&needle) {
            continue;
        }

        let edited = payload[span.clone()] != original.get(span.clone()).unwrap_or_default()[..];
        let offset = mono(format!("{:0OFFSET_DIGITS$X}", string.start))
            .w(px(OFFSET_WIDTH))
            .flex_none()
            .text_color(if edited { color(EDITED) } else { color(MUTED) });

        let which = Field::Text(string.start);
        let draft = text.clone();
        let input = field(("string", string.start), &text, "", editing(editor, which), None).on_click(cx.listener(
            move |editor, _, _, cx| {
                editor.start_editing(WindowField::View(which), draft.clone());
                cx.notify();
            },
        ));
        list = list.child(div().flex().items_start().gap(px(GAP)).child(offset).child(input));
    }

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .gap(px(GAP))
        .child(header)
        .child(separator(false))
        .child(list)
        .into_any_element()
}

/// Writes the new text of the string that starts at `start`.
pub fn string_changed(editor: &mut Editor, start: usize, text: &str) -> Option<String> {
    let entry = editor.current()?;
    let mut payload = entry.payload().to_vec();
    let string = found(&payload, entry.load_arg()).into_iter().find(|string| string.start == start)?;
    let span = string.start..string.end;

    match codec::encode(encoding(string.kind), text) {
        Ok(bytes) if bytes.len() <= span.len() => {
            payload[span].fill(TERMINATOR);
            payload[start..start + bytes.len()].copy_from_slice(&bytes);
            editor.set_payload(payload);
            None
        }
        Ok(bytes) => Some(format!("Too long: {} bytes, the string has {}", bytes.len(), span.len())),
        Err(error) => Some(format!("Cannot encode: {error}")),
    }
}
