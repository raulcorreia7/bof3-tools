//! A palette bank as a grid of 16-color rows, with a 15-bit color picker.
//!
//! A color word is `stp << 15 | blue << 10 | green << 5 | red`, five bits per channel. Word `0x0000`
//! draws transparent; the STP bit marks a semi-transparent texel.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{AnyElement, Bounds, Context, Div, Pixels, div, prelude::*, px};
use rebof3_formats::clut::color_rgba;

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
/// The bytes of one color word.
const COLOR_BYTES: usize = 2;

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

/// Rewrites colour `index` of the shown entry through a change to its word.
pub type SetColor = fn(&mut Editor, usize, &dyn Fn(u16) -> u16);

/// The track areas of the three channel sliders.
pub type Sliders = [Rc<Cell<Bounds<Pixels>>>; 3];

#[derive(Default)]
pub struct State {
    selected: usize,
    sliders: Sliders,
}

/// The bank's colors.
pub fn words(bytes: &[u8]) -> Vec<u16> {
    bytes.as_chunks::<COLOR_BYTES>().0.iter().map(|pair| u16::from_le_bytes(*pair)).collect()
}

/// Writes color `index` of `bytes` as `word`. Every other byte keeps its value, a trailing odd byte
/// included. Returns false when the bank has no such color.
fn put_color(bytes: &mut [u8], index: usize, word: u16) -> bool {
    let start = index * COLOR_BYTES;
    let Some(slot) = bytes.get_mut(start..start + COLOR_BYTES) else { return false };
    slot.copy_from_slice(&word.to_le_bytes());
    true
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
        .child(picker(words[selected], selected, &editor.views.palette.sliders, set_color, cx))
        .child(separator(false))
        .child(grid)
        .into_any_element()
}

/// Channel sliders and the STP switch for color `index`, which `set` rewrites.
pub fn picker(word: u16, index: usize, sliders: &Sliders, set: SetColor, cx: &mut Context<Editor>) -> Div {
    let mut controls = div().flex().flex_col().gap(px(GAP_SMALL)).child(mono(format!("Color {index}: {word:#06X}")));

    for (at, channel) in Channel::ALL.into_iter().enumerate() {
        let value = channel.of(word);
        let control = slider(
            ("channel", at),
            u32::from(value),
            0..=u32::from(CHANNEL_MAX),
            &sliders[at],
            cx,
            move |editor, value| {
                let value = u16::try_from(value).unwrap_or(CHANNEL_MAX);
                set(editor, index, &|word| channel.with(word, value));
            },
        );
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
            set(editor, index, &|word| word ^ STP_BIT);
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
fn set_color(editor: &mut Editor, index: usize, change: &dyn Fn(u16) -> u16) {
    let Some(entry) = editor.current() else { return };
    let mut bytes = entry.payload().to_vec();
    let Some(&word) = words(&bytes).get(index) else { return };
    if put_color(&mut bytes, index, change(word)) {
        editor.set_payload(bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rebof3_formats::Format;

    #[test]
    fn a_channel_change_keeps_the_other_channels() {
        let word = 0x8000 | 3 << BLUE_SHIFT | 2 << GREEN_SHIFT | 1;

        assert_eq!(Channel::Green.of(word), 2);
        assert_eq!(Channel::Green.with(word, CHANNEL_MAX), 0x8000 | 3 << BLUE_SHIFT | CHANNEL_MAX << GREEN_SHIFT | 1);
    }

    #[test]
    fn a_color_write_keeps_every_other_byte_and_a_trailing_odd_one() {
        let mut bytes = vec![0x11, 0x22, 0x33, 0x44, 0x55];
        assert!(put_color(&mut bytes, 1, 0xBEEF));
        assert_eq!(bytes, [0x11, 0x22, 0xEF, 0xBE, 0x55]);
        assert!(!put_color(&mut bytes, 2, 0), "the odd byte is not a color");
        assert_eq!(words(&bytes), [0x2211, 0xBEEF]);
    }

    /// Every color of every retail palette bank takes an STP flip in its own two bytes and flips back to
    /// the stored bytes. Point `BOF3_DISC_DIR` at an extracted disc; without it the test skips.
    #[test]
    fn every_retail_color_edit_rewrites_only_its_word() {
        let Some(disc) = crate::retail_support::retail_disc() else { return };
        let folder = std::env::temp_dir().join(format!("bof3-edit-palette-{}", std::process::id()));
        let mut project = crate::project::Project::open(disc.as_ref(), &folder).expect("project opens");

        let (mut banks, mut failures) = (0, Vec::new());
        for path in project.archives.clone() {
            let archive = project.load(&path).expect("archive loads");
            for index in (0..archive.entries().len()).filter(|&index| archive.formats[index] == Format::ClutBank) {
                banks += 1;
                let payload = archive.original(index);
                let mut bytes = payload.to_vec();
                for (color, word) in words(payload).into_iter().enumerate() {
                    put_color(&mut bytes, color, word ^ STP_BIT);
                    let changed = (0..bytes.len()).filter(|&at| bytes[at] != payload[at]).count();
                    put_color(&mut bytes, color, word);
                    if changed != 1 {
                        failures.push(format!("{}#{index} color {color}: {changed} bytes changed", path.0));
                    }
                }
                if bytes != payload {
                    failures.push(format!("{}#{index}: the flips do not restore the bank", path.0));
                }
            }
        }

        let _ = std::fs::remove_dir_all(&folder);
        eprintln!("palette: {} problems over {banks} banks", failures.len());
        assert!(banks > 0 && failures.is_empty(), "{:?}", failures.iter().take(8).collect::<Vec<_>>());
    }
}
