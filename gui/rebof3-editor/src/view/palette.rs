//! A palette bank as a grid of 16-color rows, with a 15-bit color picker.
//!
//! A color word is `stp << 15 | blue << 10 | green << 5 | red`, five bits per channel. Word `0x0000`
//! draws transparent; the STP bit marks a semi-transparent texel.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{AnyElement, Bounds, Context, Div, Pixels, div, prelude::*, px};
use rebof3_assets::decode::color_rgba;

use super::Entry;
use crate::app::Editor;
use crate::theme::{CORNER, MUTED, color};
use crate::widgets::{GAP, GAP_SMALL, checkbox, mono, muted, outline, separator, slider, swatch, tip};

/// Colors in one row of a 4-bit palette, the grid's width.
const ROW_COLORS: usize = 16;
const CHANNEL_BITS: u32 = 5;
const CHANNEL_MAX: u16 = (1 << CHANNEL_BITS) - 1;
const GREEN_SHIFT: u32 = CHANNEL_BITS;
const BLUE_SHIFT: u32 = 2 * CHANNEL_BITS;
const STP_BIT: u16 = 1 << 15;

/// The side of one swatch in the grid, and of the picker's preview, in pixels.
pub const SWATCH: f32 = 18.0;
const PREVIEW: f32 = 3.0 * SWATCH;
/// Space between two swatches of a row, in pixels.
pub const SWATCH_GAP: f32 = 1.0;
/// Width of a row number and of a channel label, in pixels.
const ROW_LABEL_WIDTH: f32 = 40.0;
const CHANNEL_LABEL_WIDTH: f32 = 50.0;

/// A channel of a color word.
#[derive(Clone, Copy)]
enum Channel {
    Red,
    Green,
    Blue,
}

impl Channel {
    const ALL: [Channel; 3] = [Channel::Red, Channel::Green, Channel::Blue];

    fn shift(self) -> u32 {
        match self {
            Channel::Red => 0,
            Channel::Green => GREEN_SHIFT,
            Channel::Blue => BLUE_SHIFT,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Channel::Red => "red",
            Channel::Green => "green",
            Channel::Blue => "blue",
        }
    }

    fn of(self, word: u16) -> u16 {
        word >> self.shift() & CHANNEL_MAX
    }

    fn with(self, word: u16, value: u16) -> u16 {
        word & !(CHANNEL_MAX << self.shift()) | value.min(CHANNEL_MAX) << self.shift()
    }
}

#[derive(Default)]
pub struct State {
    selected: usize,
    sliders: [Rc<Cell<Bounds<Pixels>>>; 3],
}

/// The bank's colors.
pub fn words(bytes: &[u8]) -> Vec<u16> {
    bytes.as_chunks::<2>().0.iter().map(|pair| u16::from_le_bytes(*pair)).collect()
}

fn bytes(words: &[u16]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}

pub fn show(editor: &Editor, entry: &Entry<'_>, cx: &mut Context<Editor>) -> AnyElement {
    let words = words(entry.payload());
    if words.is_empty() {
        return muted("The bank holds no colors.").into_any_element();
    }
    let original = entry.original();
    let selected = editor.views.palette.selected.min(words.len() - 1);

    let mut grid = div().id("palette-grid").flex().flex_col().flex_1().gap(px(SWATCH_GAP)).overflow_y_scroll();
    for (row, colors) in words.chunks(ROW_COLORS).enumerate() {
        let mut line = div().flex().items_center().gap(px(SWATCH_GAP));
        line = line.child(mono(format!("{row:>4}")).w(px(ROW_LABEL_WIDTH)).text_color(color(MUTED)));
        for (column, &word) in colors.iter().enumerate() {
            let index = row * ROW_COLORS + column;
            let edited = original.get(index * 2..index * 2 + 2) != Some(&word.to_le_bytes()[..]);
            line = line.child(
                swatch(("color", index), color_rgba(word), SWATCH, outline(index == selected, edited))
                    .tooltip(tip(format!("{index}: {word:#06X}")))
                    .on_click(cx.listener(move |editor, _, _, cx| {
                        editor.views.palette.selected = index;
                        cx.notify();
                    })),
            );
        }
        grid = grid.child(line);
    }

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .gap(px(GAP))
        .child(mono(format!("{} colors, {} rows of {ROW_COLORS}", words.len(), words.len().div_ceil(ROW_COLORS))))
        .child(picker(editor, words[selected], selected, cx))
        .child(separator(false))
        .child(grid)
        .into_any_element()
}

/// Channel sliders and the STP switch for color `index`.
fn picker(editor: &Editor, word: u16, index: usize, cx: &mut Context<Editor>) -> Div {
    let mut controls = div().flex().flex_col().gap(px(GAP_SMALL)).child(mono(format!("Color {index}: {word:#06X}")));

    for (at, channel) in Channel::ALL.into_iter().enumerate() {
        let value = channel.of(word);
        let bounds = &editor.views.palette.sliders[at];
        let control =
            slider(("channel", at), u32::from(value), 0..=u32::from(CHANNEL_MAX), bounds, cx, move |editor, value| {
                let value = u16::try_from(value).unwrap_or(CHANNEL_MAX);
                set_color(editor, index, |word| channel.with(word, value));
            });
        controls = controls.child(
            div()
                .flex()
                .items_center()
                .gap(px(GAP))
                .child(div().w(px(CHANNEL_LABEL_WIDTH)).child(channel.label()))
                .child(control)
                .child(mono(value.to_string())),
        );
    }

    let stp = word & STP_BIT != 0;
    controls = controls.child(checkbox("stp", "semi-transparent (STP)", stp).on_click(cx.listener(
        move |editor, _, _, cx| {
            set_color(editor, index, |word| word ^ STP_BIT);
            cx.notify();
        },
    )));

    div()
        .flex()
        .flex_none()
        .gap(px(GAP))
        .child(swatch("preview", color_rgba(word), PREVIEW, None).rounded(px(CORNER)))
        .child(controls)
}

/// Rewrites color `index` of the shown bank.
fn set_color(editor: &mut Editor, index: usize, change: impl FnOnce(u16) -> u16) {
    let Some(entry) = editor.current() else { return };
    let mut words = words(entry.payload());
    let Some(word) = words.get_mut(index) else { return };
    *word = change(*word);
    editor.set_payload(bytes(&words));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_channel_change_keeps_the_other_channels() {
        let word = 0x8000 | 3 << BLUE_SHIFT | 2 << GREEN_SHIFT | 1;

        assert_eq!(Channel::Green.of(word), 2);
        assert_eq!(Channel::Green.with(word, CHANNEL_MAX), 0x8000 | 3 << BLUE_SHIFT | CHANNEL_MAX << GREEN_SHIFT | 1);
    }
}
