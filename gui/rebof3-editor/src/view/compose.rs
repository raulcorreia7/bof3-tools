//! A message table as the player sees it: each message drawn in the game's font and window, page by page,
//! with the findings of `rebof3_formats::text::check` and an editor that rebuilds the table.
//!
//! The font is read from `ETC/FIRST.EMI` (`rebof3_formats::text::font`). The preview is the window alone over a
//! black backdrop, since the field behind it is the game's. A message edit is encoded, put in the table and the table
//! rebuilt; it is refused when the text cannot be encoded or the table would outgrow its space.

use std::rc::Rc;
use std::sync::Arc;

use gpui::{AnyElement, Context, Image, ImageFormat, ObjectFit, div, img, prelude::*, px};
use psx_formats::gfx::png::rgba_png;
use rebof3_emi::text::codec::{self, Encoding};
use rebof3_formats::message::table;
use rebof3_formats::text::check::{self, Problem};
use rebof3_formats::text::font::Font;
use rebof3_formats::text::layout::{self, Context as LayoutContext, Layout, PageEnd};
use rebof3_formats::text::render::{self, Rgba};

use super::{Entry, Field, input};
use crate::app::Editor;
use crate::project::{ArchivePath, Project};
use crate::theme::{EDITED, MUTED, color};
use crate::widgets::{GAP, GAP_SMALL, button, mono, muted, selectable, strong};

/// The archive that holds the font: sheet, text palette and frame palette entries.
const FONT_ARCHIVE: &str = "BIN/ETC/FIRST.EMI";
const SHEET: usize = 3;
const TEXT_PALETTE: usize = 8;
const FRAME_PALETTE: usize = 9;
/// Preview pixels per game pixel.
const SCALE: f32 = 2.0;
/// The backdrop behind the window.
const BACKDROP: [u8; 3] = [0, 0, 0];
const LIST_WIDTH: f32 = 220.0;
/// Glyphs of a message shown in its list row.
const ROW_GLYPHS: usize = 28;

#[derive(Default)]
pub struct State {
    font: Option<Result<Rc<Font>, String>>,
    pub selected: usize,
    pub page: usize,
    shown: Option<Rc<Shown>>,
}

/// What the preview shows, and what it was built from.
struct Shown {
    key: (String, usize, u64, usize),
    layout: Layout,
    window: Option<Arc<Image>>,
    window_size: (usize, usize),
    answers: Option<(Arc<Image>, (usize, usize))>,
    problems: Vec<Problem>,
}

fn png(image: &Rgba) -> (Arc<Image>, (usize, usize)) {
    let bytes = rgba_png(image.width as u32, image.height as u32, &image.pixels);
    (Arc::new(Image::from_bytes(ImageFormat::Png, bytes)), (image.width, image.height))
}

fn load_font(project: &mut Project) -> Result<Font, String> {
    let loaded = project.load(&ArchivePath(FONT_ARCHIVE.into()))?;
    let payload = |index: usize| project.payload(&loaded, index).to_vec();
    Font::new(&payload(SHEET), project.load_arg(&loaded, SHEET), &payload(TEXT_PALETTE), &payload(FRAME_PALETTE))
        .ok_or_else(|| "the font sheet in FIRST.EMI does not decode".to_string())
}

/// Loads the font once and builds the preview of the selected message and page when either changed.
pub fn prepare(editor: &mut Editor) {
    if editor.views.compose.font.is_none() {
        let Some(project) = editor.project.as_mut() else { return };
        editor.views.compose.font = Some(load_font(project).map(Rc::new));
    }
    let Some(Ok(font)) = editor.views.compose.font.clone() else { return };
    let Some(entry) = editor.current() else { return };
    let Ok(parsed) = table::parse(entry.payload()) else { return };
    let state = &editor.views.compose;
    let selected = state.selected.min(parsed.messages.len().saturating_sub(1));
    let key = (entry.archive.path.0.clone(), entry.index, entry.project.revision(), selected);
    let page = state.page;
    if state.shown.as_ref().is_some_and(|shown| shown.key == key) {
        return;
    }
    let Some(message) = parsed.messages.get(selected) else { return };
    let laid = layout::layout(message, LayoutContext { table: Some(entry.payload()), runtime: None });
    let problems = check::problems(&laid, &font);
    let shown_page = laid.pages.get(page.min(laid.pages.len() - 1));
    let (window, window_size) = shown_page
        .map(|page| {
            let size = laid.placement.kind.size();
            png(&render::window(&font, page, size, laid.placement.framed, usize::MAX, BACKDROP))
        })
        .unzip();
    let answers = shown_page.and_then(|page| match &page.end {
        PageEnd::Choice(choice) => Some(png(&render::choice(&font, choice, BACKDROP))),
        _ => None,
    });
    let shown = Shown { key, layout: laid, window, window_size: window_size.unwrap_or_default(), answers, problems };
    editor.views.compose.shown = Some(Rc::new(shown));
}

/// The compose view of the table in `entry`.
pub fn show(editor: &Editor, entry: &Entry<'_>, cx: &mut Context<Editor>) -> AnyElement {
    let state = &editor.views.compose;
    let payload = entry.payload();
    let Ok(parsed) = table::parse(payload) else {
        return muted("This payload is not a message table.").into_any_element();
    };
    if let Some(Err(error)) = &state.font {
        return muted(format!("The font did not load: {error}")).into_any_element();
    }
    let selected = state.selected.min(parsed.messages.len().saturating_sub(1));
    let needle = editor.views.text.filter.to_lowercase();

    let mut list = div().id("compose-list").flex().flex_col().w(px(LIST_WIDTH)).flex_none().overflow_y_scroll();
    for (index, message) in parsed.messages.iter().enumerate() {
        let text = codec::decode(Encoding::Message, message);
        if !needle.is_empty() && !text.to_lowercase().contains(&needle) {
            continue;
        }
        let head: String = text.chars().take(ROW_GLYPHS).collect();
        list = list.child(
            selectable(("compose-message", index), index == selected)
                .child(mono(format!("{index:3} {head}")).whitespace_nowrap())
                .on_click(cx.listener(move |editor, _, _, cx| {
                    editor.views.compose.selected = index;
                    editor.views.compose.page = 0;
                    cx.notify();
                })),
        );
    }

    let Some(shown) = state.shown.as_ref().filter(|shown| shown.key.3 == selected) else {
        return div().flex().gap(px(GAP)).child(list).into_any_element();
    };
    let pages = shown.layout.pages.len();
    let page = state.page.min(pages - 1);
    let text =
        parsed.messages.get(selected).map(|message| codec::decode(Encoding::Message, message)).unwrap_or_default();
    let used = payload.len();

    let step = |id: &'static str, label: &'static str, enabled: bool, next: bool, cx: &mut Context<Editor>| {
        button(id, label, enabled).on_click(cx.listener(move |editor, _, _, cx| {
            let page = &mut editor.views.compose.page;
            *page = if next { *page + 1 } else { page.saturating_sub(1) };
            editor.views.compose.shown = None;
            cx.notify();
        }))
    };
    let mut preview = div().flex().flex_col().gap(px(GAP_SMALL)).child(
        div()
            .flex()
            .items_center()
            .gap(px(GAP))
            .child(strong(format!("Message {selected}, page {} of {pages}", page + 1)))
            .child(step("compose-prev", "Previous", page > 0, false, cx))
            .child(step("compose-next", "Next", page + 1 < pages, true, cx))
            .child(muted(format!(
                "table {used} of {} bytes{}",
                table::LIMIT,
                shown.layout.placement.operand.map_or(String::new(), |operand| format!(", window {operand:02X}"))
            ))),
    );
    if let Some(window) = &shown.window {
        let (w, h) = shown.window_size;
        preview = preview
            .child(img(Arc::clone(window)).w(px(w as f32 * SCALE)).h(px(h as f32 * SCALE)).object_fit(ObjectFit::Fill));
    }
    if let Some((answers, (w, h))) = &shown.answers {
        preview = preview.child(muted("Answers")).child(
            img(Arc::clone(answers)).w(px(*w as f32 * SCALE)).h(px(*h as f32 * SCALE)).object_fit(ObjectFit::Fill),
        );
    }
    let field = input(editor, ("compose-text", selected), Field::Message(selected), &text, "", None, cx);
    preview = preview.child(field);
    for problem in shown.problems.iter().filter(|problem| problem.page == page) {
        preview = preview.child(div().text_color(color(EDITED)).child(problem.text.clone()));
    }
    preview = preview.child(
        div().text_color(color(MUTED)).child(
            "Portraits are not drawn: no message code selects a face sheet in the game's text code; {BOX} picks the window's place only.",
        ),
    );
    div().flex().gap(px(GAP)).child(list).child(preview).into_any_element()
}

/// Rewrites message `index` of the shown table from `text`. Refuses text the codec cannot encode, an empty message
/// and a table that would outgrow its space.
pub fn message_changed(editor: &mut Editor, index: usize, text: &str) -> Option<String> {
    let entry = editor.current()?;
    let mut parsed = match table::parse(entry.payload()) {
        Ok(parsed) => parsed,
        Err(error) => return Some(error.to_string()),
    };
    let bytes = match codec::encode(Encoding::Message, text) {
        Ok(bytes) if bytes.is_empty() => return Some("A message cannot be empty.".into()),
        Ok(bytes) => bytes,
        Err(error) => return Some(format!("Cannot encode: {error}")),
    };
    let slot = parsed.messages.get_mut(index)?;
    if *slot == bytes {
        return None;
    }
    *slot = bytes;
    match table::build_within_limit(&parsed) {
        Ok(rebuilt) => {
            editor.set_payload(rebuilt);
            editor.views.compose.shown = None;
            None
        }
        Err(error) => Some(format!("Refused: {error}")),
    }
}
