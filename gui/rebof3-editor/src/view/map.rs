//! An area map bundle seen from above: one square per cell. A cell's hue is its texture page and its
//! brightness the mean of its four corner heights; a cell with no tile is left out.
//!
//! The textured mode draws each cell's tile instead, read from the VRAM of the area's own scene
//! (`rebof3_emi::replay::scenes::field`, the project's edits included) through the tile descriptor
//! (`rebof3_formats::map::terrain::top_down`).
//!
//! A picked cell's corner heights are editable. The edit decodes the bundle, changes the one cell and
//! encodes it again ([`MapBundle::encode`]), so every header offset follows and an unchanged bundle
//! writes back byte for byte.

use std::cell::Cell as Shared;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{AnyElement, Bounds, Context, Div, Image, ImageFormat, MouseButton, MouseDownEvent, MouseMoveEvent, ObjectFit, Pixels, Point, canvas, div, img, prelude::*, px};
use rebof3_emi::replay::scenes;
use rebof3_formats::map::terrain::{self, mesh_of_bundle};
use rebof3_formats::map::{CELL_BYTES, MapBundle};

use super::texture::paint_pixels;
use super::{Entry, Field, input};
use crate::app::Editor;
use crate::theme::{BACKGROUND, MUTED, color};
use crate::widgets::{GAP, GAP_SMALL, button, mono, muted, separator, strong, tab};

/// Screen pixels per map cell, drawn flat.
const CELL_PIXELS: f32 = 10.0;
/// Screen pixels per map cell, textured: one per texel.
const TEXTURED_PIXELS: f32 = terrain::CELL_PIXELS as f32;
/// Width of a corner height field, in pixels.
const CORNER_WIDTH: f32 = 56.0;
/// Width of the picked cell's panel, in pixels.
const PANEL_WIDTH: f32 = 220.0;
/// The alpha of a cell with no tile, and of a drawn cell.
const EMPTY: u8 = 0;
const OPAQUE: u8 = 0xFF;
/// The darkest and brightest a cell's colour gets, as a fraction of its page's hue.
const LOW_SHADE: f32 = 0.35;
const HIGH_SHADE: f32 = 1.0;
/// One hue per page selector (bits 28-29 of the tile descriptor).
const PAGE_HUES: [[u8; 3]; 4] = [[0x6F, 0xB8, 0x5A], [0x5A, 0x8F, 0xD0], [0xD0, 0xA0, 0x4F], [0xB8, 0x6A, 0xC8]];
/// The corners in stored order, as the mesh builder places them.
const CORNER_NAMES: [&str; CELL_BYTES] = ["near", "east", "south", "far"];

#[derive(Default)]
pub struct State {
    /// The archive, entry and cell whose corners the panel edits.
    picked: Option<(String, usize, usize)>,
    /// The cell under the pointer.
    hover: Option<usize>,
    image: Rc<Shared<Bounds<Pixels>>>,
    /// Draw the cells flat, by texture page and height, instead of textured.
    flat: bool,
    /// The textured view of the shown entry, when its area's scene replays.
    textured: Option<Rc<Textured>>,
}

/// A map bundle drawn with its tiles, and what it was drawn from.
pub struct Textured {
    /// The archive, entry and project revision the view read.
    key: (String, usize, u64),
    /// The view as a PNG, which the window draws as one image and the export writes.
    image: Arc<Image>,
    summary: String,
}

impl State {
    /// What the textured view read, once it is drawn.
    #[cfg(test)]
    pub fn textured_summary(&self) -> Option<String> {
        self.textured.as_ref().map(|textured| textured.summary.clone())
    }
}

/// Draws the shown map bundle with its tiles, once per entry and project revision.
pub fn prepare(editor: &mut Editor) {
    let Some(entry) = editor.current() else { return };
    let Some(project) = editor.project.as_ref() else { return };
    let key = (entry.archive.path.0.clone(), entry.index, project.revision());
    if editor.views.map.textured.as_ref().is_some_and(|textured| textured.key == key) {
        return;
    }
    let Some(scene) = scenes::home(&key.0) else {
        editor.views.map.textured = None;
        return;
    };
    let Some(project) = editor.project.as_mut() else { return };
    let replayed = super::scene::replay(project, &scene);
    let Some(entry) = editor.current() else { return };
    let Ok(bundle) = MapBundle::decode(entry.payload()) else {
        editor.views.map.textured = None;
        return;
    };
    let view = terrain::top_down(&bundle, &replayed.vram);
    let summary = format!("Textured from the VRAM of {}: {} cells drawn, {} whose page or CLUT row the scene does not hold.", replayed.scene.name, view.textured, view.unwritten);
    let image = Arc::new(Image::from_bytes(ImageFormat::Png, view.png()));
    editor.views.map.textured = Some(Rc::new(Textured { key, image, summary }));
}

/// The mean corner height of each cell.
fn mean_heights(bundle: &MapBundle) -> Vec<i32> {
    let corners = CELL_BYTES as i32;
    bundle.cells.iter().map(|cell| cell.heights.iter().map(|&height| i32::from(height)).sum::<i32>() / corners).collect()
}

/// One RGBA pixel per cell, row-major: the cell's page hue, darker for a lower cell. A cell whose grid
/// value is 0 holds no tile and draws nothing.
pub fn cell_colors(bundle: &MapBundle) -> Vec<[u8; 4]> {
    let heights = mean_heights(bundle);
    let low = heights.iter().copied().min().unwrap_or(0);
    let high = heights.iter().copied().max().unwrap_or(0);
    let span = (high - low).max(1) as f32;
    let selectors = mesh_of_bundle(bundle).selectors;

    (0..bundle.cells.len())
        .map(|cell| {
            if bundle.grid.get(cell).copied().unwrap_or(0) == 0 {
                return [0, 0, 0, EMPTY];
            }
            let shade = LOW_SHADE + (HIGH_SHADE - LOW_SHADE) * (heights[cell] - low) as f32 / span;
            let [red, green, blue] = PAGE_HUES[usize::from(selectors[cell]) % PAGE_HUES.len()];
            let scale = |channel: u8| (f32::from(channel) * shade).round() as u8;
            [scale(red), scale(green), scale(blue), OPAQUE]
        })
        .collect()
}

/// A one-line summary of the bundle's sections.
pub fn summary(bundle: &MapBundle) -> String {
    let tiles: usize = bundle.rects.iter().map(Vec::len).sum::<usize>() + bundle.quads.iter().map(Vec::len).sum::<usize>();
    format!(
        "{} × {} cells · {} tile descriptors · {} tile records · lists {} + {} · {} objects · {} record cells · \
         scroll seat {:#06X} · height table {}",
        bundle.width,
        bundle.height,
        bundle.records.len(),
        tiles,
        bundle.list_a.len(),
        bundle.list_b.len(),
        bundle.objects.len(),
        bundle.cells_24.len(),
        bundle.scroll_seat,
        if bundle.heights.is_some() { "shipped" } else { "built at load" },
    )
}

pub fn show(editor: &Editor, entry: &Entry<'_>, cx: &mut Context<Editor>) -> AnyElement {
    let bundle = match MapBundle::decode(entry.payload()) {
        Ok(bundle) => bundle,
        Err(error) => return muted(format!("The map bundle does not decode: {error}")).into_any_element(),
    };
    let state = &editor.views.map;
    let width = usize::from(bundle.width);
    let height = usize::from(bundle.height);
    let textured = textured(editor, entry);
    let cell = cell_pixels(editor, entry);

    let measure = Rc::clone(&state.image);
    let image = match textured.clone() {
        // One texel per screen pixel, drawn as one image: a quad per texel would be too many.
        Some(textured) => {
            div().relative().child(img(Arc::clone(&textured.image)).size_full().object_fit(ObjectFit::Fill)).child(canvas(move |area, _, _| measure.set(area), |_, (), _, _| {}).absolute().size_full())
        }
        None => {
            let pixels = cell_colors(&bundle);
            div().child(canvas(move |area, _, _| measure.set(area), move |area, (), window, _| paint_pixels(window, area, &pixels, width, CELL_PIXELS)).size_full())
        }
    }
    .w(px(width as f32 * cell))
    .h(px(height as f32 * cell))
    .flex_none();

    let hover = match state.hover {
        Some(cell) => describe(&bundle, cell),
        None => "Click a cell to edit its corner heights.".into(),
    };

    let map = div()
        .id("map")
        .flex_1()
        .min_h_0()
        .overflow_scroll()
        .bg(color(BACKGROUND))
        .child(image)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|editor, event: &MouseDownEvent, _, cx| {
                editor.views.map.picked = picked_at(editor, event.position);
                editor.stop_editing();
                cx.notify();
            }),
        )
        .on_mouse_move(cx.listener(|editor, event: &MouseMoveEvent, _, cx| {
            editor.views.map.hover = cell_at(editor, event.position);
            cx.notify();
        }));

    let picked = picked(editor, entry);
    let body = div().flex().flex_1().min_h_0().gap(px(GAP)).child(map).child(panel(editor, &bundle, picked, cx));
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .gap(px(GAP))
        .child(muted(summary(&bundle)))
        .child(modes(editor, entry, cx))
        .child(separator(false))
        .child(mono(hover).text_color(color(MUTED)))
        .child(body)
        .into_any_element()
}

/// The textured view of the shown entry, unless the flat mode is chosen or the view is stale.
fn textured(editor: &Editor, entry: &Entry<'_>) -> Option<Rc<Textured>> {
    let state = &editor.views.map;
    let textured = state.textured.as_ref().filter(|_| !state.flat)?;
    let (archive, index, revision) = &textured.key;
    (*archive == entry.archive.path.0 && *index == entry.index && *revision == entry.project.revision()).then(|| Rc::clone(textured))
}

/// Screen pixels per cell in the mode drawn.
fn cell_pixels(editor: &Editor, entry: &Entry<'_>) -> f32 {
    if textured(editor, entry).is_some() { TEXTURED_PIXELS } else { CELL_PIXELS }
}

/// The mode tabs, what the textured view read, and its PNG export.
fn modes(editor: &Editor, entry: &Entry<'_>, cx: &mut Context<Editor>) -> Div {
    let available = editor.views.map.textured.is_some();
    let shown = textured(editor, entry);
    let mut row = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(GAP_SMALL))
        .child(tab("map-textured", "Textured", shown.is_some()).on_click(cx.listener(|editor, _, _, cx| {
            editor.views.map.flat = false;
            cx.notify();
        })))
        .child(tab("map-flat", "Pages and heights", shown.is_none()).on_click(cx.listener(|editor, _, _, cx| {
            editor.views.map.flat = true;
            cx.notify();
        })));
    match shown {
        Some(textured) => {
            let name = format!("{}-{}.png", entry.archive.path.0.replace('/', "_"), entry.index);
            let image = Arc::clone(&textured.image);
            row = row
                .child(separator(true))
                .child(muted(textured.summary.clone()))
                .child(button("map-export", "Export PNG", true).on_click(cx.listener(move |editor, _, _, cx| editor.save_file(name.clone(), Ok(image.bytes.clone()), cx))));
        }
        None if !available => {
            row = row.child(muted("No scene loads this archive, so its tiles cannot be read from VRAM."));
        }
        None => {}
    }
    row
}

/// What the hover line says about `cell`.
fn describe(bundle: &MapBundle, cell: usize) -> String {
    let width = usize::from(bundle.width).max(1);
    let grid = bundle.grid.get(cell).copied().unwrap_or(0);
    let descriptor = bundle.records.get(usize::from(grid)).filter(|_| grid != 0);
    let heights = bundle.cells.get(cell).map(|cell| cell.heights).unwrap_or_default();
    let code = bundle.codes.get(cell).copied().unwrap_or(0);
    format!("x {}  y {}  heights {heights:?}  code {code:#04X}  grid {grid}  tile {}", cell % width, cell / width, descriptor.map_or_else(|| "none".into(), |word| format!("{word:#010X}")),)
}

/// The picked cell's corner heights, as fields.
fn panel(editor: &Editor, bundle: &MapBundle, picked: Option<usize>, cx: &mut Context<Editor>) -> Div {
    let mut panel = div().flex().flex_col().flex_none().w(px(PANEL_WIDTH)).gap(px(GAP_SMALL));
    let Some(cell) = picked.filter(|&cell| cell < bundle.cells.len()) else {
        return panel.child(muted("No cell picked."));
    };
    let width = usize::from(bundle.width).max(1);
    panel = panel.child(strong(format!("Cell x {} y {}", cell % width, cell / width)));

    for (corner, name) in CORNER_NAMES.into_iter().enumerate() {
        let text = bundle.cells[cell].heights[corner].to_string();
        let height = input(editor, ("corner", corner), Field::MapCorner(corner), &text, "", Some(CORNER_WIDTH), cx);
        panel = panel.child(div().flex().items_center().gap(px(GAP)).child(div().w(px(CORNER_WIDTH)).child(name)).child(height));
    }
    panel.child(muted("A corner height is a signed byte, -128 to 127; the mesh scales it by 16."))
}

/// The picked cell of the shown entry.
fn picked(editor: &Editor, entry: &Entry<'_>) -> Option<usize> {
    let (archive, index, cell) = editor.views.map.picked.as_ref()?;
    (*archive == entry.archive.path.0 && *index == entry.index).then_some(*cell)
}

/// The shown entry and the cell under `position`.
fn picked_at(editor: &Editor, position: Point<Pixels>) -> Option<(String, usize, usize)> {
    let entry = editor.current()?;
    Some((entry.archive.path.0.clone(), entry.index, cell_at(editor, position)?))
}

/// The cell under `position`.
fn cell_at(editor: &Editor, position: Point<Pixels>) -> Option<usize> {
    let entry = editor.current()?;
    let (width, height) = (entry.payload().first().copied()?, entry.payload().get(1).copied()?);
    let cell = cell_pixels(editor, &entry);
    let area = editor.views.map.image.get();
    let (x, y) = (position.x - area.origin.x, position.y - area.origin.y);
    if x < Pixels::ZERO || y < Pixels::ZERO {
        return None;
    }
    let (column, row) = ((x / px(cell)) as usize, (y / px(cell)) as usize);
    (column < usize::from(width) && row < usize::from(height)).then_some(row * usize::from(width) + column)
}

/// The payload with corner `corner` of `cell` set to `height`.
pub fn with_corner(payload: &[u8], cell: usize, corner: usize, height: i8) -> Result<Vec<u8>, String> {
    let mut bundle = MapBundle::decode(payload).map_err(|error| error.to_string())?;
    let record = bundle.cells.get_mut(cell).ok_or("no such cell")?;
    record.heights[corner] = height;
    Ok(bundle.encode())
}

/// Writes a corner height the user typed. Returns a status message when it cannot be written.
pub fn corner_changed(editor: &mut Editor, corner: usize, text: &str) -> Option<String> {
    let cell = picked(editor, &editor.current()?)?;
    let Ok(height) = text.trim().parse::<i8>() else {
        return Some("A corner height is a number from -128 to 127.".into());
    };
    let entry = editor.current()?;
    match with_corner(entry.payload(), cell, corner, height) {
        Ok(payload) => {
            editor.set_payload(payload);
            None
        }
        Err(error) => Some(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rebof3_formats::map::Cell;

    /// A 2 × 1 bundle: a low cell with a tile and a high cell without one.
    fn bundle() -> MapBundle {
        MapBundle {
            width: 2,
            height: 1,
            cells: vec![Cell { heights: [0; CELL_BYTES] }, Cell { heights: [8; CELL_BYTES] }],
            codes: vec![0; 4],
            grid: vec![1, 0],
            records: vec![0, 0],
            ..MapBundle::default()
        }
    }

    #[test]
    fn a_cell_without_a_tile_draws_nothing() {
        let colors = cell_colors(&bundle());
        assert_eq!(colors.len(), 2);
        assert_eq!(colors[0][3], OPAQUE);
        assert_eq!(colors[1][3], EMPTY);
    }

    #[test]
    fn a_higher_cell_is_brighter() {
        let mut bundle = bundle();
        bundle.grid = vec![1, 1];
        let colors = cell_colors(&bundle);
        assert!(colors[1][1] > colors[0][1], "{colors:?}");
    }

    #[test]
    fn a_corner_edit_changes_only_that_corner() {
        let payload = bundle().encode();
        assert_eq!(MapBundle::decode(&payload).unwrap().encode(), payload, "an unchanged bundle writes back as is");

        let edited = MapBundle::decode(&with_corner(&payload, 1, 2, -3).unwrap()).unwrap();
        let mut expected = bundle();
        expected.cells[1].heights[2] = -3;
        assert_eq!(edited, expected);
        assert!(with_corner(&payload, 9, 0, 0).is_err());
    }
}
