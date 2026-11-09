//! A scene's whole VRAM, replayed from its archives (`rebof3_emi::replay`).
//!
//! The frame buffer draws as 16-bit words, with an outline around each region and the entry that
//! wrote it under the pointer. Picking a region and a depth decodes it from VRAM with any CLUT row
//! the scene holds, so a texture can be read with a palette another archive loaded. The scene is
//! one of the default resident sets (`rebof3_emi::replay::scenes`); it starts as the scene that
//! loads the shown entry.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    AnyElement, Bounds, Context, Div, MouseButton, MouseDownEvent, MouseMoveEvent, Pixels, Point, SharedString, canvas,
    div, fill, point, prelude::*, px, rgba, size,
};
use psx_formats::gfx::png;
use psx_formats::gfx::texture::Clut;
use psx_formats::gfx::vram::{VRAM_HEIGHT, VRAM_WIDTH};
use rebof3_emi::replay::scenes::{self, Kind as SceneKind, Scene};
use rebof3_emi::replay::{Kind, Region, Vram};
use rebof3_formats::clut::color_rgba;

use super::Entry;
use super::scene::{self, Replayed};
use super::texture::{Bits, paint_pixels};
use crate::app::Editor;
use crate::theme::{BACKGROUND, BORDER, MUTED, color};
use crate::widgets::{GAP, GAP_SMALL, button, mono, muted, separator, slider, strong, tab};

const ZOOM_MIN: u32 = 1;
const ZOOM_MAX: u32 = 4;
/// Screen pixels per texel of the decoded region.
const DECODED_ZOOM: f32 = 2.0;
/// Outline width of a region, in screen pixels.
const OUTLINE: f32 = 1.0;
/// Outline colours: an upload, palette words, the picked region.
const UPLOAD_LINE: u32 = 0x4FC3_F7A0;
const PALETTE_LINE: u32 = 0xFFB7_4DA0;
const PICKED_LINE: u32 = 0xFFFF_FFFF;
/// Words per CLUT column step.
const CLUT_STEP: usize = 16;

#[derive(Default)]
pub struct State {
    /// The chosen scene; `None` follows the shown entry's archive.
    chosen: Option<Scene>,
    catalog: Vec<Scene>,
    replayed: Option<Rc<Replayed>>,
    zoom: u32,
    hover: Option<(usize, usize)>,
    picked: Option<Region>,
    bits: Option<Bits>,
    clut: Option<Clut>,
    /// The next click in VRAM picks the CLUT instead of a region.
    choosing_clut: bool,
    zoom_slider: Rc<Cell<Bounds<Pixels>>>,
    image: Rc<Cell<Bounds<Pixels>>>,
}

/// The scene shown for `archive` when none is chosen: the one that loads it, else the frontend.
fn default_scene(archive: &str) -> Scene {
    scenes::home(archive).unwrap_or_else(scenes::frontend)
}

impl State {
    /// The scene replay shown, once `prepare` has run.
    pub(super) fn replayed(&self) -> Option<Rc<Replayed>> {
        self.replayed.clone()
    }

    /// The zoom of the scene views, once `prepare` has run.
    pub(super) fn zoom(&self) -> Option<u32> {
        (self.zoom != 0).then_some(self.zoom)
    }
}

/// Replays the shown scene when it or the project changed.
pub fn prepare(editor: &mut Editor) {
    let Some(archive) = editor.archive.as_ref().map(|archive| archive.path.0.clone()) else { return };
    let Some(project) = editor.project.as_mut() else { return };
    let state = &mut editor.views.vram;
    if state.catalog.is_empty() {
        state.catalog = scene::catalog(project);
    }
    if state.zoom == 0 {
        state.zoom = ZOOM_MIN;
    }
    let wanted = state.chosen.clone().unwrap_or_else(|| default_scene(&archive));
    if state.replayed.as_ref().is_some_and(|replayed| replayed.is_current(&wanted, project)) {
        return;
    }
    state.replayed = Some(scene::replay(project, &wanted));
}

/// The RGBA view of every VRAM word.
fn pixels(vram: &Vram) -> Vec<[u8; 4]> {
    vram.words().iter().map(|&word| color_rgba(word)).collect()
}

/// The RGBA pixels of a region decoded at `bits` with the CLUT at `clut`, and the width in texels.
fn decode(vram: &Vram, region: &Region, bits: Bits, clut: Option<Clut>) -> (Vec<[u8; 4]>, usize) {
    let (x, y, width, height) =
        (usize::from(region.x), usize::from(region.y), usize::from(region.width), usize::from(region.height));
    match bits.depth() {
        None => {
            let pixels = (y..y + height)
                .flat_map(|row| (x..x + width).map(move |column| (column, row)))
                .map(|(column, row)| color_rgba(vram.word(column, row)))
                .collect();
            (pixels, width)
        }
        Some(depth) => {
            let colors = clut.map(|clut| vram.clut_words(clut, depth)).unwrap_or_default();
            let pixels = vram
                .texels(x, y, width, height, depth)
                .into_iter()
                .map(|index| colors.get(usize::from(index)).map_or([0; 4], |&word| color_rgba(word)))
                .collect();
            (pixels, width * depth.texels_per_word())
        }
    }
}

/// The VRAM as an RGBA PNG, one pixel per word.
pub fn vram_png(vram: &Vram) -> Vec<u8> {
    let bytes: Vec<u8> = pixels(vram).into_iter().flatten().collect();
    png::rgba_png(VRAM_WIDTH as u32, VRAM_HEIGHT as u32, &bytes)
}

fn describe(region: &Region) -> String {
    let kind = match region.kind {
        Kind::Upload => "upload",
        Kind::Palette => "palette",
        Kind::Image => "image",
    };
    format!("{} {kind}, x {} y {}, {}×{} words", region.writer, region.x, region.y, region.width, region.height)
}

pub fn show(editor: &Editor, _entry: &Entry<'_>, cx: &mut Context<Editor>) -> AnyElement {
    let state = &editor.views.vram;
    let Some(replayed) = state.replayed.clone() else {
        return muted("The scene has not been replayed yet.").into_any_element();
    };
    let vram = &replayed.vram;
    let zoom = state.zoom.max(ZOOM_MIN) as f32;

    let words = pixels(vram);
    let outlines: Vec<(Region, u32)> = replayed
        .regions
        .iter()
        .map(|region| {
            let line = match region.kind {
                Kind::Palette => PALETTE_LINE,
                _ => UPLOAD_LINE,
            };
            (region.clone(), line)
        })
        .chain(state.picked.clone().map(|region| (region, PICKED_LINE)))
        .collect();
    let measure = Rc::clone(&state.image);
    let image = canvas(
        move |area, _, _| measure.set(area),
        move |area, (), window, _| {
            paint_pixels(window, area, &words, VRAM_WIDTH, zoom);
            for (region, line) in &outlines {
                paint_outline(window, area, region, zoom, *line);
            }
        },
    )
    .w(px(VRAM_WIDTH as f32 * zoom))
    .h(px(VRAM_HEIGHT as f32 * zoom))
    .flex_none();

    let hover = match state.hover {
        Some((x, y)) => {
            let writer = vram.writer(x, y).map_or("nothing".to_string(), ToString::to_string);
            format!("x {x}  y {y}  word {:#06X}  written by {writer}", vram.word(x, y))
        }
        None if state.choosing_clut => "Click a palette row to use it as the CLUT.".into(),
        None => "Click a region to decode it.".into(),
    };

    let canvas_area = div()
        .id("vram")
        .flex_1()
        .min_h_0()
        .overflow_scroll()
        .bg(color(BACKGROUND))
        .child(image)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|editor, event: &MouseDownEvent, _, cx| {
                click(editor, event.position);
                cx.notify();
            }),
        )
        .on_mouse_move(cx.listener(|editor, event: &MouseMoveEvent, _, cx| {
            editor.views.vram.hover = word_at(editor, event.position);
            cx.notify();
        }));

    let mut column = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .gap(px(GAP))
        .child(scene_controls(editor, &replayed, cx))
        .child(separator(false));
    if !vram.missing().is_empty() {
        column = column.child(muted(format!("Not on the disc, skipped: {}", vram.missing().join(", "))));
    }
    column = column.child(mono(hover).text_color(color(MUTED))).child(canvas_area);
    if let Some(region) = state.picked.clone() {
        column = column.child(separator(false)).child(decode_panel(editor, vram, &region, cx));
    }
    column.into_any_element()
}

/// Draws a one-pixel outline around `region`.
fn paint_outline(window: &mut gpui::Window, area: Bounds<Pixels>, region: &Region, zoom: f32, line: u32) {
    let origin = point(area.origin.x + px(f32::from(region.x) * zoom), area.origin.y + px(f32::from(region.y) * zoom));
    let (width, height) = (px(f32::from(region.width) * zoom), px(f32::from(region.height) * zoom));
    let thin = px(OUTLINE);
    let colour = rgba(line);
    for (at, extent) in [
        (origin, size(width, thin)),
        (point(origin.x, origin.y + height - thin), size(width, thin)),
        (origin, size(thin, height)),
        (point(origin.x + width - thin, origin.y), size(thin, height)),
    ] {
        window.paint_quad(fill(Bounds::new(at, extent), colour));
    }
}

/// The VRAM word under `position`.
fn word_at(editor: &Editor, position: Point<Pixels>) -> Option<(usize, usize)> {
    let state = &editor.views.vram;
    let area = state.image.get();
    let zoom = px(state.zoom.max(ZOOM_MIN) as f32);
    let (x, y) = (position.x - area.origin.x, position.y - area.origin.y);
    if x < Pixels::ZERO || y < Pixels::ZERO {
        return None;
    }
    let (x, y) = ((x / zoom) as usize, (y / zoom) as usize);
    (x < VRAM_WIDTH && y < VRAM_HEIGHT).then_some((x, y))
}

/// Picks the CLUT or the region under `position`.
fn click(editor: &mut Editor, position: Point<Pixels>) {
    let Some((x, y)) = word_at(editor, position) else { return };
    let state = &mut editor.views.vram;
    if state.choosing_clut {
        state.clut = Clut::new((x - x % CLUT_STEP) as u16, y as u16);
        state.choosing_clut = false;
        return;
    }
    let Some(replayed) = state.replayed.as_ref() else { return };
    let inside = |region: &&Region| {
        (usize::from(region.x)..usize::from(region.x) + usize::from(region.width)).contains(&x)
            && (usize::from(region.y)..usize::from(region.y) + usize::from(region.height)).contains(&y)
    };
    state.picked = replayed.regions.iter().find(inside).cloned();
    if state.bits.is_none() {
        state.bits = Some(Bits::Four);
    }
}

/// The scene kind tabs, the step through the kind's scenes, zoom and export.
pub(super) fn scene_controls(editor: &Editor, replayed: &Replayed, cx: &mut Context<Editor>) -> Div {
    let state = &editor.views.vram;
    let current = &replayed.scene;
    let mut kinds: Vec<SceneKind> = state.catalog.iter().map(|scene| scene.kind).collect();
    kinds.sort();
    kinds.dedup();

    let mut row = div().flex().flex_wrap().items_center().gap(px(GAP_SMALL)).child("Scene");
    for kind in kinds {
        row = row.child(tab(("scene-kind", kind as usize), kind.label(), current.kind == kind).on_click(cx.listener(
            move |editor, _, _, cx| {
                let first = editor.views.vram.catalog.iter().find(|scene| scene.kind == kind).cloned();
                editor.views.vram.chosen = first;
                editor.views.vram.picked = None;
                cx.notify();
            },
        )));
    }
    let step = |offset: isize| {
        move |editor: &mut Editor, _: &gpui::ClickEvent, _: &mut gpui::Window, cx: &mut Context<Editor>| {
            let state = &mut editor.views.vram;
            let Some(current) = state.replayed.as_ref().map(|replayed| replayed.scene.clone()) else { return };
            let same: Vec<&Scene> = state.catalog.iter().filter(|scene| scene.kind == current.kind).collect();
            let at = same.iter().position(|scene| **scene == current).unwrap_or(0) as isize;
            let next = (at + offset).rem_euclid(same.len().max(1) as isize) as usize;
            state.chosen = same.get(next).map(|scene| (*scene).clone());
            state.picked = None;
            cx.notify();
        }
    };
    row = row
        .child(separator(true))
        .child(button("scene-previous", "◀", true).on_click(cx.listener(step(-1))))
        .child(strong(current.name.clone()))
        .child(button("scene-next", "▶", true).on_click(cx.listener(step(1))))
        .child(button("scene-home", "This entry's scene", state.chosen.is_some()).on_click(cx.listener(
            |editor, _, _, cx| {
                editor.views.vram.chosen = None;
                editor.views.vram.picked = None;
                cx.notify();
            },
        )));
    let zoom =
        slider("vram-zoom", state.zoom.max(ZOOM_MIN), ZOOM_MIN..=ZOOM_MAX, &state.zoom_slider, cx, |editor, zoom| {
            editor.views.vram.zoom = zoom;
        });
    let name = format!("{}.png", current.name.replace([':', ' ', '+'], "_"));
    row.child(separator(true))
        .child("Zoom")
        .child(zoom)
        .child(mono(format!("{}×", state.zoom.max(ZOOM_MIN))))
        .child(separator(true))
        .child(button("vram-export", "Export PNG", true).on_click(cx.listener(move |editor, _, _, cx| {
            // Encode on click: a PNG of the whole VRAM is too slow to build on every frame.
            let Some(replayed) = editor.views.vram.replayed.clone() else { return };
            editor.save_file(name.clone(), Ok(vram_png(&replayed.vram)), cx);
        })))
}

/// The picked region, its depth and CLUT, and the region decoded from VRAM.
fn decode_panel(editor: &Editor, vram: &Vram, region: &Region, cx: &mut Context<Editor>) -> Div {
    let state = &editor.views.vram;
    let bits = state.bits.unwrap_or(Bits::Four);
    let mut controls = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(GAP_SMALL))
        .child(mono(describe(region)))
        .child(separator(true))
        .child("Depth");
    for (choice, label) in Bits::ALL {
        controls =
            controls.child(tab(SharedString::from(format!("vram-depth-{label}")), label, bits == choice).on_click(
                cx.listener(move |editor, _, _, cx| {
                    editor.views.vram.bits = Some(choice);
                    cx.notify();
                }),
            ));
    }
    if let Some(depth) = bits.depth() {
        let clut = match state.clut {
            Some(clut) => {
                let writer = vram.clut_writer(clut, depth).map_or("no single entry".into(), ToString::to_string);
                format!("CLUT ({}, {}) from {writer}", clut.x, clut.y)
            }
            None => "no CLUT".into(),
        };
        controls = controls.child(separator(true)).child(mono(clut)).child(
            button("vram-pick-clut", if state.choosing_clut { "Click a row…" } else { "Pick CLUT" }, true).on_click(
                cx.listener(|editor, _, _, cx| {
                    editor.views.vram.choosing_clut = !editor.views.vram.choosing_clut;
                    cx.notify();
                }),
            ),
        );
    }

    let (pixels, width) = decode(vram, region, bits, state.clut);
    let height = usize::from(region.height);
    let decoded =
        canvas(|_, _, _| {}, move |area, (), window, _| paint_pixels(window, area, &pixels, width, DECODED_ZOOM))
            .w(px(width as f32 * DECODED_ZOOM))
            .h(px(height as f32 * DECODED_ZOOM))
            .flex_none();
    div().flex().flex_col().flex_none().max_h(px(360.0)).gap(px(GAP_SMALL)).child(controls).child(
        div()
            .id("vram-decoded")
            .overflow_scroll()
            .border_1()
            .border_color(color(BORDER))
            .bg(color(BACKGROUND))
            .child(decoded),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rebof3_emi::Entry as TableEntry;
    use rebof3_emi::replay::Writer;

    fn sample() -> Vram {
        let mut vram = Vram::new();
        let sheet: Vec<u8> = (0..1024u16).flat_map(|_| 0x2121u16.to_le_bytes()).collect();
        let bank: Vec<u8> = (0..16u16).flat_map(|word| (0x8000 | word).to_le_bytes()).collect();
        let entries = [
            TableEntry { size: sheet.len() as u32, offset: 0, ram_ptr: 1 << 8, first4: 0, file_type: 3 },
            TableEntry { size: bank.len() as u32, offset: 0, ram_ptr: 0x8003_3800, first4: 0, file_type: 0 },
        ];
        vram.load("BIN/A.EMI", &entries, &[&sheet, &bank]);
        vram.reset_palette();
        vram.upload_palette();
        vram
    }

    #[test]
    fn a_region_decodes_through_a_scene_clut() {
        let vram = sample();
        let region = vram.regions().into_iter().find(|region| region.kind == Kind::Upload).expect("the upload");
        assert_eq!(region.writer, Writer::entry("BIN/A.EMI", 0));
        let clut = Clut::new(0, 480);
        let (pixels, width) = decode(&vram, &region, Bits::Four, clut);
        assert_eq!(width, 128);
        // Index 1 reads palette word 0x8001; index 2 reads 0x8002.
        assert_eq!(pixels[0], color_rgba(0x8001));
        assert_eq!(pixels[1], color_rgba(0x8002));
        let (direct, width) = decode(&vram, &region, Bits::Sixteen, None);
        assert_eq!((direct[0], width), (color_rgba(0x2121), 32));
    }

    #[test]
    fn the_export_is_a_whole_vram_png() {
        let png = vram_png(&sample());
        assert_eq!(&png[..8], &png::SIGNATURE);
        match png::read(&png).expect("a PNG") {
            png::Picture::Rgba(picture) => assert_eq!((picture.width, picture.height), (1024, 512)),
            png::Picture::Indexed(_) => panic!("an RGBA PNG"),
        }
    }

    /// AREA024's scene replays from the disc with the project's edits: the literal site's page and
    /// row come from the area's entries 7 and 8, and an edit to the bank's first colour shows in VRAM
    /// at the next replay. Point `BOF3_DISC_DIR` at an extracted disc; without it the test skips.
    #[test]
    fn a_retail_scene_replays_with_the_projects_edits() {
        use crate::project::{ArchivePath, Project};
        let Some(disc) = crate::retail_support::retail_disc() else { return };
        let folder = std::env::temp_dir().join(format!("bof3-edit-vram-{}", std::process::id()));
        let mut project = Project::open(disc.as_ref(), &folder).expect("project opens");
        let area = "BIN/WORLD00/AREA024.EMI";
        let scene = default_scene(area);
        let replayed = super::scene::replay(&mut project, &scene);
        assert!(replayed.vram.missing().is_empty());
        assert_eq!(replayed.vram.writer(704, 256), Some(&Writer::entry(area, 7)));
        let clut = Clut::new(160, 483).expect("a CLUT");
        assert!(
            replayed.joins.iter().any(|join| join.texture == Writer::entry(area, 7) && join.clut == clut),
            "the AREA024 site joins entry 7 to row (160, 483)"
        );

        let loaded = project.load(&ArchivePath(area.into())).expect("the area loads");
        let mut bank = project.payload(&loaded, 8).to_vec();
        let first = usize::from(clut.x) * 2 + (usize::from(clut.y) - 483) * 512;
        bank[first..first + 2].copy_from_slice(&0x7C1Fu16.to_le_bytes());
        project.set(&loaded, 8, bank);
        assert!(!replayed.is_current(&scene, &project), "an edit makes the replay stale");
        let again = super::scene::replay(&mut project, &scene);
        assert_eq!(again.vram.word(160, 483), 0x7C1F);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
