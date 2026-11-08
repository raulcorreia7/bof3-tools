//! The strings of an entry: a message bank's records, or the strings a scan finds in any other
//! payload.
//!
//! A string is rewritten in place. New text must encode to no more bytes than the old string took;
//! the rest of its span is filled with terminators, so no offset table or pointer moves.

use gpui::{AnyElement, Context, div, prelude::*, px};
use rebof3_emi::text::codec::{self, Encoding};
use rebof3_emi::text::discover::{self, Found, Kind};

use super::{Entry, Field, editing, message};
use crate::app::{Editor, Field as WindowField};
use crate::theme::{EDITED, MUTED, color};
use crate::widgets::{GAP, GAP_SMALL, field, mono, muted, separator, strong, tab};

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
    /// Whether the strings show as message windows instead of editable text.
    pub preview: bool,
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
        .child(mode_tab("Edit", false, state.preview, cx))
        .child(mode_tab("Preview", true, state.preview, cx))
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

        if state.preview {
            let window = message::window(&text);
            list = list.child(div().flex().items_start().gap(px(GAP)).child(offset).child(window));
            continue;
        }

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

/// The tab that shows the strings as text (`preview` false) or as message windows.
fn mode_tab(label: &'static str, preview: bool, shown: bool, cx: &mut Context<Editor>) -> impl IntoElement {
    tab(("text-mode", usize::from(preview)), label, preview == shown).on_click(cx.listener(move |editor, _, _, cx| {
        editor.views.text.preview = preview;
        cx.notify();
    }))
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    /// The glyphs of a decoded string, with every `{TAG}` taken out.
    fn glyphs(text: &str) -> String {
        let mut out = String::new();
        let mut rest = text;
        while let Some(c) = rest.chars().next() {
            if c == '{'
                && let Some(close) = rest.find('}')
            {
                rest = &rest[close + 1..];
                continue;
            }
            out.push(c);
            rest = &rest[c.len_utf8()..];
        }
        out
    }

    /// Every retail string lays out as message windows, and the windows print every glyph of the string in
    /// order. Point `BOF3_DISC_DIR` at an extracted disc; without it the test skips.
    #[test]
    fn every_retail_string_prints_all_its_glyphs() {
        let Some(disc) = std::env::var_os("BOF3_DISC_DIR") else { return eprintln!("skipping: set BOF3_DISC_DIR") };
        let folder = std::env::temp_dir().join(format!("bof3-edit-preview-{}", std::process::id()));
        let mut project = crate::project::Project::open(disc.as_ref(), &folder).expect("project opens");

        let (mut strings, mut failures, mut colors) = (0, Vec::new(), BTreeMap::<String, usize>::new());
        for path in project.archives.clone() {
            let archive = project.load(&path).expect("archive loads");
            for index in 0..archive.entries().len() {
                let entry = Entry { project: &project, archive: &archive, index };
                let payload = entry.payload();
                for string in found(payload, entry.load_arg()) {
                    let text = codec::decode(encoding(string.kind), &payload[string.start..string.end]);
                    if text.contains("{BACKSPACE}") {
                        continue;
                    }
                    strings += 1;
                    for (at, _) in text.match_indices("{COLOR:") {
                        *colors.entry(text[at + 7..at + 9].to_string()).or_default() += 1;
                    }

                    let printed: String = message::pages(&text)
                        .iter()
                        .flatten()
                        .flatten()
                        .filter(|run| !run.placeholder)
                        .map(|run| run.text.as_str())
                        .collect();
                    if printed != glyphs(&text) {
                        failures.push(format!("{}#{index}@{:X}: {text}", path.0, string.start));
                    }
                }
            }
        }

        let _ = std::fs::remove_dir_all(&folder);
        eprintln!("strings: {} of {strings} fail; colour operands {colors:?}", failures.len());
        assert!(strings > 0 && failures.is_empty(), "{:?}", failures.iter().take(8).collect::<Vec<_>>());
    }
}
