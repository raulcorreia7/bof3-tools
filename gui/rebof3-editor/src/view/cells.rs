//! An area's packed 4-bit cell grid (`0x80032000`) drawn as the map's cells, one colour per value.
//!
//! The grid holds one nibble per map cell, at linear index `x + width * y`: the high nibble for an even
//! index, the low nibble for an odd one (`emi/etc/game/00@0x801C6FD4`, `wiki: formats/area`). The
//! width and height come from the map bundle of the same archive. Painting writes one nibble and
//! keeps every other byte.

use std::cell::Cell as Shared;
use std::rc::Rc;

use gpui::{
    AnyElement, Bounds, Context, Div, MouseButton, MouseDownEvent, MouseMoveEvent, Pixels, Point, canvas, div,
    prelude::*, px,
};
use rebof3_formats::Format;

use super::Entry;
use super::texture::paint_pixels;
use crate::app::Editor;
use crate::theme::{BACKGROUND, MUTED, color};
use crate::widgets::{GAP, GAP_SMALL, mono, muted, separator, swatch, tab};

/// Screen pixels per map cell.
const CELL_PIXELS: f32 = 10.0;
/// Values a nibble holds.
const VALUES: usize = 16;
/// The side of an ink swatch, in pixels.
const INK_SWATCH: f32 = 18.0;
/// The value of a cell no battler stands on (`func_801C7518`), which draws nothing.
const FREE: u8 = 0;
const OPAQUE: u8 = 0xFF;
/// One colour per nibble value; value 0, a cell no battler stands on, draws nothing.
const VALUE_COLOURS: [[u8; 3]; VALUES] = [
    [0x00, 0x00, 0x00],
    [0xE0, 0x6C, 0x5A],
    [0x5A, 0xA8, 0xE0],
    [0x7C, 0xC8, 0x6A],
    [0xE0, 0xC0, 0x4A],
    [0xB0, 0x78, 0xE0],
    [0x4A, 0xD0, 0xC0],
    [0xE0, 0x8C, 0xC8],
    [0xA0, 0xA0, 0xA0],
    [0xC8, 0x50, 0x50],
    [0x50, 0x70, 0xC8],
    [0x50, 0xA0, 0x50],
    [0xC8, 0xA0, 0x30],
    [0x80, 0x50, 0xB0],
    [0x30, 0x90, 0x90],
    [0xFF, 0xFF, 0xFF],
];

#[derive(Default)]
pub struct State {
    /// The value the pencil paints.
    ink: u8,
    /// The cell under the pointer.
    hover: Option<(usize, usize)>,
    image: Rc<Shared<Bounds<Pixels>>>,
}

/// The map's width and height, from the map bundle header of the same archive.
pub(super) fn map_size(entry: &Entry<'_>) -> Option<(usize, usize)> {
    let bundle = entry.archive.formats.iter().position(|format| *format == Format::AreaMapBundle)?;
    let header = entry.sibling(bundle);
    Some((usize::from(*header.first()?), usize::from(*header.get(1)?)))
}

/// The value of cell `(x, y)` of a grid `width` cells wide, or 0 past the grid.
pub fn value(grid: &[u8], width: usize, x: usize, y: usize) -> u8 {
    let index = x + width * y;
    let Some(&byte) = grid.get(index >> 1) else { return FREE };
    if index & 1 == 1 { byte & 0x0F } else { byte >> 4 }
}

/// The grid with cell `(x, y)` set to `value`, or `None` when the cell is past the grid.
pub fn with_value(grid: &[u8], width: usize, x: usize, y: usize, value: u8) -> Option<Vec<u8>> {
    let index = x + width * y;
    let mut grid = grid.to_vec();
    let byte = grid.get_mut(index >> 1)?;
    let value = value & 0x0F;
    *byte = if index & 1 == 1 { (*byte & 0xF0) | value } else { (*byte & 0x0F) | (value << 4) };
    Some(grid)
}

/// One RGBA pixel per cell, row-major.
pub fn cell_colors(grid: &[u8], width: usize, height: usize) -> Vec<[u8; 4]> {
    let mut pixels = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            let value = value(grid, width, x, y);
            let [red, green, blue] = VALUE_COLOURS[usize::from(value)];
            pixels.push([red, green, blue, if value == FREE { 0 } else { OPAQUE }]);
        }
    }
    pixels
}

pub fn show(editor: &Editor, entry: &Entry<'_>, cx: &mut Context<Editor>) -> AnyElement {
    let Some((width, height)) = map_size(entry) else {
        return muted("No map bundle in this archive gives the grid's width and height.").into_any_element();
    };
    let state = &editor.views.cells;
    let pixels = cell_colors(entry.payload(), width, height);

    let measure = Rc::clone(&state.image);
    let image = canvas(
        move |area, _, _| measure.set(area),
        move |area, (), window, _| paint_pixels(window, area, &pixels, width, CELL_PIXELS),
    )
    .w(px(width as f32 * CELL_PIXELS))
    .h(px(height as f32 * CELL_PIXELS))
    .flex_none();

    let hover = match state.hover {
        Some((x, y)) => format!("x {x}  y {y}  value {}", value(entry.payload(), width, x, y)),
        None => "Left-click or drag paints the ink; right-click picks it.".into(),
    };

    let grid = div()
        .id("cells")
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
                if let Some(entry) = editor.current()
                    && let (Some((width, _)), Some((x, y))) = (map_size(&entry), cell_at(editor, event.position))
                {
                    editor.views.cells.ink = value(entry.payload(), width, x, y);
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
        .child(muted(format!("{width} × {height} cells, one 4-bit value each; 0 holds no battler, 1 to 12 hold battlers of reach 0 to 2 (func_801C7518)")))
        .child(inks(state.ink, cx))
        .child(separator(false))
        .child(mono(hover).text_color(color(MUTED)))
        .child(grid)
        .into_any_element()
}

/// The sixteen values as ink swatches.
fn inks(ink: u8, cx: &mut Context<Editor>) -> Div {
    let mut row = div().flex().items_center().gap(px(GAP_SMALL)).child("Ink");
    for value in 0..VALUES as u8 {
        let [red, green, blue] = VALUE_COLOURS[usize::from(value)];
        let picked = value == ink;
        row = row.child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .child(swatch(("ink", usize::from(value)), [red, green, blue, OPAQUE], INK_SWATCH, None))
                .child(tab(("ink-value", usize::from(value)), value.to_string(), picked).on_click(cx.listener(
                    move |editor, _, _, cx| {
                        editor.views.cells.ink = value;
                        cx.notify();
                    },
                ))),
        );
    }
    row
}

/// The cell under `position`.
fn cell_at(editor: &Editor, position: Point<Pixels>) -> Option<(usize, usize)> {
    let entry = editor.current()?;
    let (width, height) = map_size(&entry)?;
    let area = editor.views.cells.image.get();
    let (x, y) = (position.x - area.origin.x, position.y - area.origin.y);
    if x < Pixels::ZERO || y < Pixels::ZERO {
        return None;
    }
    let (x, y) = ((x / px(CELL_PIXELS)) as usize, (y / px(CELL_PIXELS)) as usize);
    (x < width && y < height).then_some((x, y))
}

/// Tracks the pointer, and paints the ink at it when `paints`.
fn pointer(editor: &mut Editor, position: Point<Pixels>, paints: bool) {
    let hover = cell_at(editor, position);
    editor.views.cells.hover = hover;
    let Some((x, y)) = hover.filter(|_| paints) else { return };
    let Some(entry) = editor.current() else { return };
    let Some((width, _)) = map_size(&entry) else { return };
    if let Some(grid) = with_value(entry.payload(), width, x, y, editor.views.cells.ink) {
        editor.set_payload(grid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nibble_follows_the_linear_index() {
        // Width 7: cell (2, 1) is index 9, the low nibble of byte 4.
        let mut grid = vec![0; 8];
        grid[4] = 0x5A;
        assert_eq!(value(&grid, 7, 2, 1), 0x0A);
        assert_eq!(value(&grid, 7, 1, 1), 0x05);
        assert_eq!(value(&grid, 7, 6, 9), FREE, "past the grid reads 0");
    }

    #[test]
    fn a_paint_changes_one_nibble() {
        let grid = vec![0x12, 0x34];
        assert_eq!(with_value(&grid, 2, 1, 0, 0xF).unwrap(), [0x1F, 0x34]);
        assert_eq!(with_value(&grid, 2, 0, 1, 0x9).unwrap(), [0x12, 0x94]);
        assert_eq!(with_value(&grid, 2, 0, 2, 1), None);
    }

    #[test]
    fn a_free_cell_draws_nothing() {
        let colors = cell_colors(&[0x01], 2, 1);
        assert_eq!(colors[0][3], 0);
        assert_eq!(colors[1][3], OPAQUE);
    }

    /// Every retail cell grid finds its map size and covers the map, and repainting a cell with its own
    /// value keeps the payload. Point `BOF3_DISC_DIR` at an extracted disc; without it the test skips.
    #[test]
    fn every_retail_grid_covers_its_map() {
        let Some(disc) = crate::retail_support::retail_disc() else { return };
        let folder = std::env::temp_dir().join(format!("bof3-edit-cells-{}", std::process::id()));
        let mut project = crate::project::Project::open(disc.as_ref(), &folder).expect("project opens");

        let (mut grids, mut failures) = (0, Vec::new());
        for path in project.archives.clone() {
            let archive = project.load(&path).expect("archive loads");
            for index in 0..archive.entries().len() {
                if archive.formats[index] != Format::AreaCellGrid {
                    continue;
                }
                grids += 1;
                let entry = Entry { project: &project, archive: &archive, index };
                let grid = entry.payload();
                let fits = map_size(&entry).filter(|(width, height)| width * height <= 2 * grid.len());
                let Some((width, height)) = fits else {
                    failures.push(format!("{}#{index}: no map size, or the map is past the grid", path.0));
                    continue;
                };
                let (x, y) = (width - 1, height - 1);
                if with_value(grid, width, x, y, value(grid, width, x, y)).as_deref() != Some(grid) {
                    failures.push(format!("{}#{index}: a repaint changes bytes", path.0));
                }
            }
        }

        let _ = std::fs::remove_dir_all(&folder);
        eprintln!("cell grids: {} of {grids} fail", failures.len());
        assert!(grids > 0 && failures.is_empty(), "{:?}", failures.iter().take(8).collect::<Vec<_>>());
    }
}
