//! A texture upload as a paintable image.
//!
//! The upload's VRAM words read at 4, 8 or 16 bits per texel. A 4- or 8-bit texture is drawn with a
//! palette row: one a rule joins to it in the scene that loads it (`rebof3_emi::texture::scene`),
//! a suggested row of that scene (`rebof3_emi::texture::suggest`), a row of a palette bank in the
//! same archive, or a grey ramp. A suggestion is listed, never chosen for the user. Painting writes
//! texel indices (or 16-bit colors) back into the words and the words back into the upload's
//! rectangles.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    AnyElement, Bounds, Context, Div, MouseButton, MouseDownEvent, MouseMoveEvent, Pixels, Point, anchored, canvas,
    deferred, div, fill, point, prelude::*, px, rgba, size,
};
use psx_formats::gfx::texture::{Clut, Depth, texel_indices, texel_words};
use psx_formats::gfx::vram;
use rebof3_assets::decode::{TextureWords, color_rgba, joined_rows, texture_words};
use rebof3_emi::replay::Writer;
use rebof3_emi::replay::scenes;
use rebof3_emi::texture::suggest;
use rebof3_formats::Format;

use super::palette::{SWATCH, SWATCH_GAP, words as words_of};
use super::{Entry, Menu, editing_menu};
use crate::app::{Editor, Menu as WindowMenu};
use crate::theme::{BACKGROUND, MUTED, color};
use crate::widgets::{GAP, button, menu_item, menu_panel, mono, muted, outline, separator, slider, swatch, tab, tip};

const ZOOM_MIN: u32 = 1;
const ZOOM_MAX: u32 = 16;
const ZOOM_DEFAULT: u32 = 2;
/// The brightest level of a 5-bit color channel.
const CHANNEL_MAX: usize = 31;
const GREEN_SHIFT: u32 = 5;
const BLUE_SHIFT: u32 = 10;
/// The alpha of a pixel that draws nothing.
const TRANSPARENT: u8 = 0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Bits {
    Four,
    Eight,
    Sixteen,
}

impl Bits {
    pub(super) const ALL: [(Bits, &'static str); 3] =
        [(Bits::Four, "4-bit"), (Bits::Eight, "8-bit"), (Bits::Sixteen, "16-bit")];

    pub(super) fn depth(self) -> Option<Depth> {
        match self {
            Bits::Four => Some(Depth::Four),
            Bits::Eight => Some(Depth::Eight),
            Bits::Sixteen => None,
        }
    }
}

/// The suggested rows the chooser lists.
const SUGGESTIONS: usize = 5;

/// A palette row read from the texture's scene: where it is and which entry wrote it.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub clut: Clut,
    pub depth: Depth,
    pub words: Vec<u16>,
    /// The entry that wrote the row.
    pub palette: String,
    /// Why the row is listed: the joining rule, or the suggestion's score.
    pub evidence: String,
}

/// Where the colors of a 4- or 8-bit texture come from.
#[derive(Clone, Debug, PartialEq)]
enum Source {
    /// A palette row a rule joins to the texture.
    Joined(usize),
    /// A row the suggestion ranks, which no rule names.
    Suggested(usize),
    /// Row `row` of the palette bank in entry `bank`.
    Bank {
        bank: usize,
        row: usize,
    },
    Grey,
}

pub struct State {
    key: Option<(String, usize)>,
    bits: Bits,
    source: Source,
    zoom: u32,
    /// The palette index (4/8 bit) or color word (16 bit) the pencil paints.
    ink: u16,
    joined: Vec<Row>,
    suggested: Vec<Row>,
    /// The suggested depth and its score, when no rule joins the texture.
    depth_hint: Option<(Bits, f64)>,
    /// The texel under the pointer and its value.
    hover: Option<(usize, usize, u16)>,
    zoom_slider: Rc<Cell<Bounds<Pixels>>>,
    image: Rc<Cell<Bounds<Pixels>>>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            key: None,
            bits: Bits::Four,
            source: Source::Grey,
            zoom: ZOOM_DEFAULT,
            ink: 0,
            joined: Vec::new(),
            suggested: Vec::new(),
            depth_hint: None,
            hover: None,
            zoom_slider: Rc::default(),
            image: Rc::default(),
        }
    }
}

/// Picks the depth and colors the first time an upload is shown: a joined row when the scene that
/// loads it has one. Otherwise the texture stays at 16 bits and the suggestion is only listed.
pub fn prepare(editor: &mut Editor) {
    let Some(entry) = editor.current() else { return };
    let key = (entry.archive.path.0.clone(), entry.index);
    if editor.views.texture.key.as_ref() == Some(&key) {
        return;
    }
    let (archive, index) = key.clone();
    let replayed = match (scenes::home(&archive), editor.project.as_mut()) {
        (Some(scene), Some(project)) => Some(super::scene::replay(project, &scene)),
        _ => None,
    };
    let Some(entry) = editor.current() else { return };
    let mut joined = Vec::new();
    let mut suggested = Vec::new();
    let mut depth_hint = None;
    match &replayed {
        Some(replayed) => {
            let me = Writer::entry(archive.as_str(), index);
            for join in replayed.joins.iter().filter(|join| join.texture == me) {
                joined.push(Row {
                    clut: join.clut,
                    depth: join.depth,
                    words: replayed.vram.clut_words(join.clut, join.depth),
                    palette: join.palette.to_string(),
                    evidence: format!("{}; scene {}", join.rule.evidence(), replayed.scene.name),
                });
            }
            if joined.is_empty()
                && let Ok(words) = texture_words(entry.load_arg(), entry.payload())
            {
                let hint = suggest::suggest(&replayed.vram, &words.words, words.width, words.height, &archive);
                depth_hint = hint.depths.first().map(|score| {
                    let bits = match score.depth {
                        Some(Depth::Four) => Bits::Four,
                        Some(Depth::Eight) => Bits::Eight,
                        None => Bits::Sixteen,
                    };
                    (bits, score.score)
                });
                suggested = hint
                    .cluts
                    .iter()
                    .take(SUGGESTIONS)
                    .map(|candidate| Row {
                        clut: candidate.clut,
                        depth: candidate.depth,
                        words: replayed.vram.clut_words(candidate.clut, candidate.depth),
                        palette: candidate.palette.to_string(),
                        evidence: format!(
                            "suggestion, score {:.2}, scene {}; no rule names this row",
                            candidate.score, replayed.scene.name
                        ),
                    })
                    .collect();
            }
        }
        None => {
            // No default scene loads the archive: the archive-only rule.
            let payloads: Vec<&[u8]> = (0..entry.archive.entries().len()).map(|at| entry.sibling(at)).collect();
            let rows = joined_rows(&archive, entry.archive.entries(), &payloads);
            for row in rows.get(&index).cloned().unwrap_or_default() {
                joined.push(Row {
                    clut: row.clut,
                    depth: row.depth,
                    words: row.words,
                    palette: format!("{} #{}", row.palette_archive, row.palette),
                    evidence: row.evidence,
                });
            }
        }
    }
    let state = &mut editor.views.texture;
    state.zoom = ZOOM_DEFAULT;
    state.ink = 0;
    state.hover = None;
    match joined.first() {
        Some(row) => {
            state.bits = if row.depth == Depth::Four { Bits::Four } else { Bits::Eight };
            state.source = Source::Joined(0);
        }
        None => {
            state.bits = Bits::Sixteen;
            state.source = Source::Grey;
        }
    }
    state.joined = joined;
    state.suggested = suggested;
    state.depth_hint = depth_hint;
    state.key = Some(key);
}

/// The colors a texture is drawn with.
fn palette(entry: &Entry<'_>, state: &State, depth: Depth) -> Vec<u16> {
    let colors = depth.colors();
    match &state.source {
        Source::Joined(at) => state.joined.get(*at).map(|row| row.words.clone()).unwrap_or_default(),
        Source::Suggested(at) => state.suggested.get(*at).map(|row| row.words.clone()).unwrap_or_default(),
        Source::Bank { bank, row } => {
            words_of(entry.sibling(*bank)).chunks(colors).nth(*row).map(<[u16]>::to_vec).unwrap_or_default()
        }
        Source::Grey => (0..colors).map(|index| grey(index, colors)).collect(),
    }
}

/// A 15-bit grey word for palette index `index` of `colors`.
fn grey(index: usize, colors: usize) -> u16 {
    let level = (index * CHANNEL_MAX / (colors - 1)) as u16;
    level | level << GREEN_SHIFT | level << BLUE_SHIFT
}

pub fn show(editor: &Editor, entry: &Entry<'_>, cx: &mut Context<Editor>) -> AnyElement {
    let state = &editor.views.texture;
    let words = match texture_words(entry.load_arg(), entry.payload()) {
        Ok(words) => words,
        Err(error) => return muted(format!("The upload does not decode: {error}")).into_any_element(),
    };

    let texels_per_word = state.bits.depth().map_or(1, Depth::texels_per_word);
    let width = words.width * texels_per_word;
    let height = words.height;
    let pixels = render(entry, state, &words.words).0;
    let zoom = state.zoom as f32;

    let measure = Rc::clone(&state.image);
    let image = canvas(
        move |area, _, _| measure.set(area),
        move |area, (), window, _| paint_pixels(window, area, &pixels, width, zoom),
    )
    .w(px(width as f32 * zoom))
    .h(px(height as f32 * zoom))
    .flex_none();

    let hover = match state.hover {
        Some((x, y, value)) => format!("x {x}  y {y}  value {value:#X}"),
        None => "Left-click or drag paints; right-click picks the ink.".into(),
    };

    let canvas_area = div()
        .id("texture")
        .flex_1()
        .min_h_0()
        .overflow_scroll()
        .bg(color(BACKGROUND))
        .child(image)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|editor, event: &MouseDownEvent, _, cx| {
                pointer(editor, event.position, true);
                cx.notify();
            }),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(|editor, event: &MouseDownEvent, _, cx| {
                if let Some((_, _, value)) = texel_at(editor, event.position) {
                    editor.views.texture.ink = value;
                }
                cx.notify();
            }),
        )
        .on_mouse_move(cx.listener(|editor, event: &MouseMoveEvent, _, cx| {
            let paints = event.dragging() && event.pressed_button == Some(MouseButton::Left);
            pointer(editor, event.position, paints);
            cx.notify();
        }));

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .gap(px(GAP))
        .child(controls(editor, entry, &words, cx))
        .child(separator(false))
        .child(mono(hover).text_color(color(MUTED)))
        .child(canvas_area)
        .into_any_element()
}

/// Paints the texture's pixels at `zoom` pixels per texel, one quad per run of equal pixels, only for
/// the rows the window shows.
pub(super) fn paint_pixels(
    window: &mut gpui::Window,
    area: Bounds<Pixels>,
    pixels: &[[u8; 4]],
    width: usize,
    zoom: f32,
) {
    if width == 0 {
        return;
    }
    let visible = window.content_mask().bounds;
    let texel = px(zoom);
    let first = ((visible.origin.y - area.origin.y) / texel).floor().max(0.0) as usize;
    let last = ((visible.origin.y + visible.size.height - area.origin.y) / texel).ceil().max(0.0) as usize;

    for (y, row) in pixels.chunks(width).enumerate().take(last).skip(first) {
        let mut x = 0;
        while x < row.len() {
            let start = x;
            while x < row.len() && row[x] == row[start] {
                x += 1;
            }
            let [red, green, blue, alpha] = row[start];
            if alpha == TRANSPARENT {
                continue;
            }
            let origin = point(area.origin.x + texel * start as f32, area.origin.y + texel * y as f32);
            let run = size(texel * (x - start) as f32, texel);
            window.paint_quad(fill(Bounds::new(origin, run), rgba(u32::from_be_bytes([red, green, blue, alpha]))));
        }
    }
}

/// The texel under `position`, and its value.
fn texel_at(editor: &Editor, position: Point<Pixels>) -> Option<(usize, usize, u16)> {
    let state = &editor.views.texture;
    let entry = editor.current()?;
    let words = texture_words(entry.load_arg(), entry.payload()).ok()?;
    let width = words.width * state.bits.depth().map_or(1, Depth::texels_per_word);

    let area = state.image.get();
    let texel = px(state.zoom as f32);
    let (x, y) = (position.x - area.origin.x, position.y - area.origin.y);
    if x < Pixels::ZERO || y < Pixels::ZERO {
        return None;
    }
    let (x, y) = ((x / texel) as usize, (y / texel) as usize);
    if x >= width || y >= words.height {
        return None;
    }
    let values = render(&entry, state, &words.words).1;
    Some((x, y, values[y * width + x]))
}

/// Tracks the pointer, and paints the ink at it when `paints`.
fn pointer(editor: &mut Editor, position: Point<Pixels>, paints: bool) {
    let hover = texel_at(editor, position);
    editor.views.texture.hover = hover;
    let Some((x, y, _)) = hover.filter(|_| paints) else { return };
    let Some(entry) = editor.current() else { return };
    let Ok(words) = texture_words(entry.load_arg(), entry.payload()) else { return };

    let round_trips =
        vram::payload_from_grid(words.width, words.height, &words.words).is_ok_and(|bytes| bytes == entry.payload());
    if !round_trips {
        editor.status = "This upload does not round-trip through the rectangle writer; painting is off.".into();
        return;
    }
    if let Some(bytes) = paint(&editor.views.texture, &words.words, words.width, words.height, x, y) {
        editor.set_payload(bytes);
    }
}

/// The RGBA pixels of the texture, and the raw value (index or word) under each pixel.
fn render(entry: &Entry<'_>, state: &State, words: &[u16]) -> (Vec<[u8; 4]>, Vec<u16>) {
    match state.bits.depth() {
        None => (words.iter().map(|&word| color_rgba(word)).collect(), words.to_vec()),
        Some(depth) => {
            let colors = palette(entry, state, depth);
            let indices = texel_indices(words, depth);
            let pixels = indices
                .iter()
                .map(|&index| colors.get(usize::from(index)).map_or([0; 4], |&word| color_rgba(word)))
                .collect();
            (pixels, indices.iter().map(|&index| u16::from(index)).collect())
        }
    }
}

/// Writes the ink at texel (`x`, `y`) and returns the new payload, or `None` when nothing changed.
fn paint(state: &State, words: &[u16], width: usize, height: usize, x: usize, y: usize) -> Option<Vec<u8>> {
    let mut words = words.to_vec();
    match state.bits.depth() {
        None => {
            let at = y * width + x;
            if words[at] == state.ink {
                return None;
            }
            words[at] = state.ink;
        }
        Some(depth) => {
            let mut indices = texel_indices(&words, depth);
            let at = y * width * depth.texels_per_word() + x;
            let ink = state.ink.min(depth.colors() as u16 - 1) as u8;
            if indices[at] == ink {
                return None;
            }
            indices[at] = ink;
            words = texel_words(&indices, depth);
        }
    }
    vram::payload_from_grid(width, height, &words).ok()
}

fn controls(editor: &Editor, entry: &Entry<'_>, words: &TextureWords, cx: &mut Context<Editor>) -> Div {
    let state = &editor.views.texture;

    let mut depth_row = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(GAP))
        .child(mono(format!("VRAM x {} y {}  {}×{} words", words.x, words.y, words.width, words.height)))
        .child(separator(true))
        .child("Depth");
    for (bits, label) in Bits::ALL {
        depth_row =
            depth_row.child(tab(label, label, state.bits == bits).on_click(cx.listener(move |editor, _, _, cx| {
                editor.views.texture.bits = bits;
                cx.notify();
            })));
    }
    let zoom = slider("zoom", state.zoom, ZOOM_MIN..=ZOOM_MAX, &state.zoom_slider, cx, |editor, zoom| {
        editor.views.texture.zoom = zoom;
    });
    depth_row = depth_row.child(separator(true)).child("Zoom").child(zoom).child(mono(format!("{}×", state.zoom)));

    let mut column = div().flex().flex_col().flex_none().gap(px(GAP)).child(depth_row);
    if let Some(hint) = suggestion(state) {
        let mut line = div().flex().flex_wrap().items_center().gap(px(GAP)).child(muted(hint));
        if let Some(row) = state.suggested.first() {
            let bits = if row.depth == Depth::Four { Bits::Four } else { Bits::Eight };
            line = line.child(button("use-suggestion", "Use", true).on_click(cx.listener(move |editor, _, _, cx| {
                editor.views.texture.bits = bits;
                editor.views.texture.source = Source::Suggested(0);
                cx.notify();
            })));
        }
        column = column.child(line);
    }
    let Some(depth) = state.bits.depth() else {
        return column.child(muted(format!("Ink: color word {:#06X} (right-click a texel to pick)", state.ink)));
    };

    let chooser = button("colors", format!("{} ▾", describe(&state.source, state)), true).on_click(cx.listener(
        |editor, _, _, cx| {
            editor.menu = Some(WindowMenu::View(Menu::Colors));
            cx.notify();
        },
    ));
    let mut chooser_column = div().flex().flex_col().child(chooser);
    if editing_menu(editor, Menu::Colors) {
        chooser_column =
            chooser_column.child(deferred(anchored().snap_to_window().child(sources(editor, entry, depth, cx))));
    }
    column = column.child(div().flex().items_center().gap(px(GAP)).child("Colors").child(chooser_column));

    let mut inks = div().flex().flex_wrap().items_center().gap(px(SWATCH_GAP)).child(div().mr(px(GAP)).child("Ink"));
    for (index, &word) in palette(entry, state, depth).iter().enumerate() {
        let selected = usize::from(state.ink) == index;
        inks = inks.child(
            swatch(("ink", index), color_rgba(word), SWATCH, outline(selected, false))
                .tooltip(tip(format!("index {index}  word {word:#06X}")))
                .on_click(cx.listener(move |editor, _, _, cx| {
                    editor.views.texture.ink = index as u16;
                    cx.notify();
                })),
        );
    }
    column.child(inks)
}

/// The open chooser of palette rows: joined rows, the rows of every palette bank, and the grey ramp.
fn sources(editor: &Editor, entry: &Entry<'_>, depth: Depth, cx: &mut Context<Editor>) -> gpui::Stateful<Div> {
    let state = &editor.views.texture;
    let mut choices: Vec<(Source, String, Option<String>)> = Vec::new();
    for (at, row) in state.joined.iter().enumerate() {
        choices.push((Source::Joined(at), label(row, "joined"), Some(row.evidence.clone())));
    }
    for (at, row) in state.suggested.iter().enumerate().filter(|(_, row)| row.depth == depth) {
        choices.push((Source::Suggested(at), label(row, "suggested"), Some(row.evidence.clone())));
    }
    for (bank, format) in entry.archive.formats.iter().enumerate() {
        if *format != Format::ClutBank {
            continue;
        }
        let rows = entry.sibling(bank).len() / 2 / depth.colors();
        for row in 0..rows {
            choices.push((Source::Bank { bank, row }, format!("bank {bank} row {row}"), None));
        }
    }
    choices.push((Source::Grey, "grey ramp".into(), None));

    let mut panel = menu_panel("sources").on_mouse_down_out(cx.listener(|editor, _, _, cx| {
        editor.menu = None;
        cx.notify();
    }));
    for (at, (source, label, evidence)) in choices.into_iter().enumerate() {
        let selected = state.source == source;
        let mut item = menu_item(("source", at), &label, if selected { "●" } else { "" }, true);
        if let Some(evidence) = evidence {
            item = item.tooltip(tip(evidence));
        }
        panel = panel.child(item.on_click(cx.listener(move |editor, _, _, cx| {
            editor.views.texture.source = source.clone();
            editor.menu = None;
            cx.notify();
        })));
    }
    panel
}

/// The suggestion line for a texture no rule joins, or `None` when one does.
fn suggestion(state: &State) -> Option<String> {
    let (bits, score) = state.depth_hint?;
    let depth = Bits::ALL.iter().find(|(choice, _)| *choice == bits).map_or("?", |(_, label)| label);
    let row = state.suggested.first().map_or(String::new(), |row| {
        format!(", then clut {},{} from {} at {}-bit", row.clut.x, row.clut.y, row.palette, row.depth.bits())
    });
    Some(format!("No rule joins this texture. Suggested, not applied: {depth} (score {score:.2}){row}."))
}

/// A scene row's name in the chooser.
fn label(row: &Row, what: &str) -> String {
    format!("{what}: {}-bit clut {},{} from {}", row.depth.bits(), row.clut.x, row.clut.y, row.palette)
}

fn describe(source: &Source, state: &State) -> String {
    match source {
        Source::Joined(at) => state.joined.get(*at).map_or("joined row".into(), |row| label(row, "joined")),
        Source::Suggested(at) => state.suggested.get(*at).map_or("suggested row".into(), |row| label(row, "suggested")),
        Source::Bank { bank, row } => format!("bank {bank} row {row}"),
        Source::Grey => "grey ramp".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grey_ramp_runs_from_black_to_white() {
        assert_eq!(grey(0, 16), 0);
        assert_eq!(grey(15, 16), 0x7FFF);
    }
}
