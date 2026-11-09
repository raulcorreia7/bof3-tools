//! The VRAM layout of a scene, in the manner of a texture-packing tool: every texture and palette placement as a
//! labelled rectangle, the collisions between them, and each entry's depth.

use std::collections::BTreeSet;
use std::rc::Rc;

use gpui::{AnyElement, Context, Div, Hsla, ParentElement, Styled, div, prelude::*, px, rgba};
use rebof3_emi::replay::{Kind as WriteKind, Writer};
use rebof3_emi::texture::{ARENA_BASE, ARENA_BYTES, ARENA_ROW_WORDS, ARENA_VRAM_ROW, ClutBlock, clut_at, sheet};
use rebof3_formats::vram::Placement;
use rebof3_formats::{Format, classify};

use super::Entry;
use super::scene::Replayed;
use super::vram as frame;
use crate::app::Editor;
use crate::project::{ArchivePath, Project};
use crate::theme::{ACCENT, BACKGROUND, BORDER, MUTED, TEXT, color};
use crate::widgets::{GAP, GAP_SMALL, button, mono, muted, selectable, separator, strong};

/// Words of the frame buffer a row holds, and its rows.
const VRAM_WIDTH: u16 = 1024;
const VRAM_HEIGHT: u16 = 512;
/// Words between the faint guide lines: a 4-bit texture page is 64 words wide.
const GUIDE_STEP: u16 = 64;
/// Words one step moves a CLUT block sideways: one 4-bit row.
const PALETTE_STEP: usize = 16;
/// Screen size of the label text and the least rectangle width that fits one, in pixels.
const LABEL_SIZE: f32 = 10.0;
const LABEL_MIN: f32 = 28.0;
/// The tallest the placement list grows before it scrolls, in pixels.
const LIST_HEIGHT: f32 = 170.0;
/// Fills, as `0xRRGGBBAA`: a texture, a CLUT block, a placement set in code, a collision.
const TEXTURE_FILL: u32 = 0x4FC3_F750;
const PALETTE_FILL: u32 = 0xFFB7_4D60;
const CODE_FILL: u32 = 0x9696_A050;
const COLLISION_FILL: u32 = 0xFF3B_3BB0;
const SELECTED_LINE: u32 = 0xFFFF_FFFF;

/// A rectangle of VRAM, in words and rows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl Rect {
    /// The part both rectangles cover.
    fn intersection(self, other: Rect) -> Option<Rect> {
        let left = self.x.max(other.x);
        let top = self.y.max(other.y);
        let right = (self.x + self.width).min(other.x + other.width);
        let bottom = (self.y + self.height).min(other.y + other.height);
        (left < right && top < bottom).then_some(Rect { x: left, y: top, width: right - left, height: bottom - top })
    }
}

/// What places a rectangle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    /// A type-3 entry: its load argument names the place.
    Texture,
    /// A type-0 CLUT block: its load address names its place in the palette arena.
    Palette,
    /// Code: nothing in the data moves it.
    Code,
}

/// One placement of the scene.
#[derive(Clone, Debug)]
pub struct Placed {
    pub writer: Writer,
    pub kind: Kind,
    /// One rectangle, or up to three for a CLUT block that starts or ends inside an arena row.
    pub rects: Vec<Rect>,
    /// The depth, and whether a draw site names it.
    pub depth: String,
    /// The load argument the project has for the entry, and the disc's, for an entry placed by data.
    pub load_arg: Option<u32>,
    pub on_disc: Option<u32>,
    /// Why a placement cannot move, or what it needs to move.
    pub note: String,
    /// The draw sites that read the placement, one line each.
    pub sites: Vec<String>,
}

impl Placed {
    pub fn moved(&self) -> bool {
        self.load_arg != self.on_disc
    }
}

/// Two placements that cover the same words.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Collision {
    pub first: usize,
    pub second: usize,
    pub area: Rect,
}

/// A direction a placement moves in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Step {
    Left,
    Right,
    Up,
    Down,
}

impl Step {
    const ALL: [(Step, &'static str); 4] = [(Step::Left, "◀"), (Step::Right, "▶"), (Step::Up, "▲"), (Step::Down, "▼")];
}

/// The rectangles a CLUT block covers in VRAM: the arena row it starts in, the whole rows after it, and the part of
/// the row it ends in.
fn palette_rects(arena_word: usize, words: usize) -> Vec<Rect> {
    let row_words = usize::from(ARENA_ROW_WORDS);
    let words = words.min((usize::from(VRAM_HEIGHT - ARENA_VRAM_ROW) * row_words).saturating_sub(arena_word));
    let (mut at, mut left) = (arena_word, words);
    let mut rects = Vec::new();
    let mut push = |at: usize, width: usize, rows: usize| {
        let place = clut_at(at);
        rects.push(Rect { x: place.x, y: place.y, width: width as u16, height: rows as u16 });
    };
    if at % row_words != 0 && left > 0 {
        let head = (row_words - at % row_words).min(left);
        push(at, head, 1);
        at += head;
        left -= head;
    }
    if left >= row_words {
        let rows = left / row_words;
        push(at, row_words, rows);
        at += rows * row_words;
        left -= rows * row_words;
    }
    if left > 0 {
        push(at, left, 1);
    }
    rects
}

/// Every placement the scene's archives make, with the project's placements applied, then the writes the replay shows
/// that no entry's data places.
pub fn placements(project: &Project, replayed: &Replayed) -> Vec<Placed> {
    let mut placed = Vec::new();
    let mut seen = BTreeSet::new();
    for path in replayed.scene.archives() {
        let Some(loaded) = project.loaded(&ArchivePath(path.to_string())) else { continue };
        for (slot, entry) in project.entries_of(loaded).iter().enumerate() {
            let writer = Writer::entry(path, slot);
            if seen.contains(&writer) {
                continue;
            }
            let payload = project.payload(loaded, slot);
            let on_disc = loaded.entries().get(slot).map(|entry| entry.ram_ptr);
            let joins = replayed.joins.iter();
            let (kind, rects, depth, sites) = match classify(entry, payload) {
                Format::VramUpload => {
                    let Some(sheet) = sheet(slot, entry.ram_ptr, payload.len()) else { continue };
                    let joined = joins.filter(|join| join.texture == writer);
                    let (depths, sites): (BTreeSet<u32>, Vec<String>) =
                        joined.map(|join| (join.depth.bits(), join.rule.evidence())).unzip();
                    (
                        Kind::Texture,
                        vec![Rect { x: sheet.x, y: sheet.y, width: sheet.width, height: sheet.height }],
                        depth_text(&depths),
                        sites,
                    )
                }
                Format::ClutBlock => {
                    let Some(block) = ClutBlock::new(slot, entry.ram_ptr, payload.len()) else { continue };
                    let joined = joins.filter(|join| join.palette == writer);
                    let (depths, sites): (BTreeSet<u32>, Vec<String>) =
                        joined.map(|join| (join.depth.bits(), join.rule.evidence())).unzip();
                    let mut depth = format!("{} colours", block.words);
                    if !depths.is_empty() {
                        depth = format!("{depth}; {}", depth_text(&depths));
                    }
                    (Kind::Palette, palette_rects(block.arena_word, block.words), depth, sites)
                }
                _ => continue,
            };
            seen.insert(writer.clone());
            let mut sites = sites;
            sites.sort();
            sites.dedup();
            let note = match kind {
                Kind::Texture => "Moves by its load argument's column and row.".to_string(),
                _ => "Moves by its load address in the palette arena.".to_string(),
            };
            placed.push(Placed { writer, kind, rects, depth, load_arg: Some(entry.ram_ptr), on_disc, note, sites });
        }
    }
    for region in &replayed.regions {
        if seen.contains(&region.writer) {
            continue;
        }
        let (depth, note) = match (&region.writer, region.kind) {
            (Writer::Named(name), _) => {
                ("unknown".to_string(), format!("Uploaded by code ({name}); no data places it."))
            }
            (_, WriteKind::Palette) => (
                "palette words".to_string(),
                "Palette words of a compressed block, copied by the loader to the arena; this view does not move them."
                    .to_string(),
            ),
            _ => (
                "unknown".to_string(),
                "Not a whole number of upload rectangles, so the loader places it itself.".to_string(),
            ),
        };
        let rect = Rect { x: region.x, y: region.y, width: region.width, height: region.height };
        placed.push(Placed {
            writer: region.writer.clone(),
            kind: Kind::Code,
            rects: vec![rect],
            depth,
            load_arg: None,
            on_disc: None,
            note,
            sites: Vec::new(),
        });
    }
    placed
}

fn depth_text(depths: &BTreeSet<u32>) -> String {
    match depths.len() {
        0 => "depth unknown: no draw site joins it".to_string(),
        _ => {
            let names: Vec<String> = depths.iter().map(|bits| format!("{bits} bpp")).collect();
            format!("{} (a draw site joins it)", names.join(" and "))
        }
    }
}

/// The overlaps between placements.
pub fn collisions(placed: &[Placed]) -> Vec<Collision> {
    let mut found = Vec::new();
    for (first, one) in placed.iter().enumerate().filter(|(_, placed)| placed.kind != Kind::Code) {
        for (second, other) in placed.iter().enumerate().skip(first + 1).filter(|(_, placed)| placed.kind != Kind::Code)
        {
            for (left, right) in one.rects.iter().flat_map(|left| other.rects.iter().map(move |right| (left, right))) {
                if let Some(area) = left.intersection(*right) {
                    found.push(Collision { first, second, area });
                }
            }
        }
    }
    found
}

/// The load argument that moves `placed` one step, or why it cannot move.
pub fn moved(project: &Project, placed: &Placed, step: Step) -> Result<u32, String> {
    let (Writer::Entry { archive, slot }, Some(load_arg)) = (&placed.writer, placed.load_arg) else {
        return Err(placed.note.clone());
    };
    let loaded = project.loaded(&ArchivePath(archive.clone())).ok_or("the archive is not loaded")?;
    let payload_bytes = project.payload(loaded, *slot).len();
    match placed.kind {
        Kind::Code => Err(placed.note.clone()),
        Kind::Texture => {
            let place = Placement::of_load_argument(load_arg);
            let (dx, dy) = match step {
                Step::Left => (-1, 0),
                Step::Right => (1, 0),
                Step::Up => (0, -1),
                Step::Down => (0, 1),
            };
            let column = i16::from(place.column) + dx;
            let row = i16::from(place.row) + dy;
            let rectangles = payload_bytes / psx_formats::gfx::vram::RECTANGLE_BYTES;
            u8::try_from(column)
                .ok()
                .zip(u8::try_from(row).ok())
                .and_then(|(column, row)| place.moved(column, row, rectangles))
                .map(|place| place.load_argument(load_arg))
                .ok_or_else(|| "a step that way leaves the frame buffer".to_string())
        }
        Kind::Palette => {
            let word = (load_arg.wrapping_sub(ARENA_BASE) / 2) as usize;
            let delta = match step {
                Step::Left => -(PALETTE_STEP as isize),
                Step::Right => PALETTE_STEP as isize,
                Step::Up => -(usize::from(ARENA_ROW_WORDS) as isize),
                Step::Down => usize::from(ARENA_ROW_WORDS) as isize,
            };
            let next =
                word.checked_add_signed(delta).filter(|next| (next + payload_bytes / 2) * 2 <= ARENA_BYTES as usize);
            next.map(|next| ARENA_BASE + 2 * next as u32)
                .ok_or_else(|| "a step that way leaves the palette arena".to_string())
        }
    }
}

/// The replay, and the placements and collisions read from it.
type Cache = (Rc<Replayed>, Rc<Vec<Placed>>, Rc<Vec<Collision>>);

#[derive(Default)]
pub struct State {
    /// The scene replay the placements were read from, and the placements.
    cache: Option<Cache>,
    selected: Option<Writer>,
}

/// Replays the shown scene, as the VRAM view does, and reads its placements when the replay changed.
pub fn prepare(editor: &mut Editor) {
    frame::prepare(editor);
    let Some(project) = editor.project.as_ref() else { return };
    let Some(replayed) = editor.views.vram.replayed() else { return };
    let current = editor.views.layout.cache.as_ref().is_some_and(|(seen, _, _)| Rc::ptr_eq(seen, &replayed));
    if current {
        return;
    }
    let placed = placements(project, &replayed);
    let found = collisions(&placed);
    let selected = editor.views.layout.selected.clone();
    // Start on the shown entry's own placement.
    let own = editor
        .archive
        .as_ref()
        .zip(editor.entry)
        .map(|(archive, slot)| Writer::entry(archive.path.0.clone(), slot))
        .filter(|writer| placed.iter().any(|placed| placed.writer == *writer));
    let selected = selected.filter(|writer| placed.iter().any(|placed| placed.writer == *writer)).or(own);
    editor.views.layout.selected = selected;
    editor.views.layout.cache = Some((replayed, Rc::new(placed), Rc::new(found)));
}

/// A short name for a writer: the archive's file and the slot.
fn short(writer: &Writer) -> String {
    match writer {
        Writer::Entry { archive, slot } => {
            let name = archive.rsplit('/').next().unwrap_or(archive).trim_end_matches(".EMI");
            format!("{name} #{slot}")
        }
        Writer::Named(name) => name.clone(),
    }
}

fn fill(kind: Kind) -> Hsla {
    Hsla::from(rgba(match kind {
        Kind::Texture => TEXTURE_FILL,
        Kind::Palette => PALETTE_FILL,
        Kind::Code => CODE_FILL,
    }))
}

pub fn show(editor: &Editor, entry: &Entry<'_>, cx: &mut Context<Editor>) -> AnyElement {
    let state = &editor.views.layout;
    let (Some((replayed, placed, found)), Some(zoom)) = (state.cache.clone(), editor.views.vram.zoom()) else {
        return muted("The scene has not been replayed yet.").into_any_element();
    };
    let zoom = zoom as f32;
    let own = Writer::entry(entry.archive.path.0.clone(), entry.index);

    let mut surface =
        div().relative().flex_none().w(px(f32::from(VRAM_WIDTH) * zoom)).h(px(f32::from(VRAM_HEIGHT) * zoom));
    surface = surface.bg(color(BACKGROUND));
    for step in (GUIDE_STEP..VRAM_WIDTH).step_by(usize::from(GUIDE_STEP)) {
        surface = surface.child(guide(f32::from(step) * zoom, true));
    }
    for step in (GUIDE_STEP..VRAM_HEIGHT).step_by(usize::from(GUIDE_STEP)) {
        surface = surface.child(guide(f32::from(step) * zoom, false));
    }
    // The palette arena reaches VRAM in one upload; its rows are outlined faintly.
    surface = surface.child(
        div()
            .absolute()
            .left(px(0.0))
            .top(px(f32::from(ARENA_VRAM_ROW) * zoom))
            .w(px(f32::from(ARENA_ROW_WORDS) * zoom))
            .h(px(f32::from(VRAM_HEIGHT - ARENA_VRAM_ROW) * zoom))
            .border_1()
            .border_color(color(BORDER)),
    );
    for (at, one) in placed.iter().enumerate() {
        let selected = state.selected.as_ref() == Some(&one.writer);
        for (part, rect) in one.rects.iter().enumerate() {
            let width = f32::from(rect.width) * zoom;
            let mut block = div()
                .id(("layout-place", at * 4 + part))
                .absolute()
                .left(px(f32::from(rect.x) * zoom))
                .top(px(f32::from(rect.y) * zoom))
                .w(px(width))
                .h(px(f32::from(rect.height) * zoom))
                .bg(fill(one.kind))
                .border_1()
                .border_color(if selected {
                    Hsla::from(rgba(SELECTED_LINE))
                } else if one.writer == own {
                    color(ACCENT)
                } else {
                    color(BORDER)
                })
                .overflow_hidden()
                .text_size(px(LABEL_SIZE))
                .text_color(color(TEXT))
                .cursor_pointer();
            if part == 0 && width >= LABEL_MIN && f32::from(rect.height) * zoom >= LABEL_SIZE {
                block = block.child(short(&one.writer));
            }
            let writer = one.writer.clone();
            surface = surface.child(block.on_click(cx.listener(move |editor, _, _, cx| {
                editor.views.layout.selected = Some(writer.clone());
                cx.notify();
            })));
        }
    }
    for collision in found.iter() {
        let area = collision.area;
        surface = surface.child(
            div()
                .absolute()
                .left(px(f32::from(area.x) * zoom))
                .top(px(f32::from(area.y) * zoom))
                .w(px(f32::from(area.width) * zoom))
                .h(px(f32::from(area.height) * zoom))
                .bg(Hsla::from(rgba(COLLISION_FILL))),
        );
    }

    let mut column = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .gap(px(GAP))
        .child(frame::scene_controls(editor, &replayed, cx))
        .child(legend(placed.len(), found.len()))
        .child(separator(false))
        .child(div().id("layout-surface").flex_1().min_h_0().overflow_scroll().child(surface));
    if !replayed.vram.missing().is_empty() {
        column = column.child(muted(format!("Not on the disc, skipped: {}", replayed.vram.missing().join(", "))));
    }
    column = column.child(separator(false)).child(list(editor, &placed, &found, cx));
    if let Some(one) = state.selected.as_ref().and_then(|writer| placed.iter().find(|placed| placed.writer == *writer))
    {
        column = column.child(detail(editor, one, &placed, &found, cx));
    }
    column.into_any_element()
}

fn guide(at: f32, vertical: bool) -> Div {
    let line = div().absolute().bg(color(BORDER));
    if vertical {
        line.left(px(at)).top(px(0.0)).w(px(1.0)).h_full()
    } else {
        line.top(px(at)).left(px(0.0)).h(px(1.0)).w_full()
    }
}

fn legend(placements: usize, collisions: usize) -> Div {
    let key = |fill_color: u32, text: &'static str| {
        div()
            .flex()
            .items_center()
            .gap(px(GAP_SMALL))
            .child(div().w(px(12.0)).h(px(12.0)).bg(Hsla::from(rgba(fill_color))))
            .child(text)
    };
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(GAP))
        .child(key(TEXTURE_FILL, "texture (load argument)"))
        .child(key(PALETTE_FILL, "CLUT block (load address)"))
        .child(key(CODE_FILL, "set in code, cannot move"))
        .child(key(COLLISION_FILL, "collision"))
        .child(mono(format!("{placements} placements, {collisions} collisions")).text_color(color(MUTED)))
}

/// One line per placement, collisions first named in the line of each side.
fn list(editor: &Editor, placed: &[Placed], found: &[Collision], cx: &mut Context<Editor>) -> impl IntoElement {
    let state = &editor.views.layout;
    let mut rows = div().id("layout-list").flex().flex_col().max_h(px(LIST_HEIGHT)).overflow_y_scroll();
    for (at, one) in placed.iter().enumerate() {
        let hits = found.iter().filter(|collision| collision.first == at || collision.second == at).count();
        let rect = one.rects.first().copied().unwrap_or(Rect { x: 0, y: 0, width: 0, height: 0 });
        let line = format!(
            "{:<22} {:<8} x {:>4} y {:>3}  {:>4}×{:<3} words  {}{}{}",
            short(&one.writer),
            match one.kind {
                Kind::Texture => "texture",
                Kind::Palette => "palette",
                Kind::Code => "code",
            },
            rect.x,
            rect.y,
            rect.width,
            rect.height,
            one.depth,
            if hits > 0 { format!("  COLLIDES x{hits}") } else { String::new() },
            if one.moved() { "  MOVED" } else { "" },
        );
        let writer = one.writer.clone();
        rows = rows.child(
            selectable(("layout-row", at), state.selected.as_ref() == Some(&one.writer)).child(mono(line)).on_click(
                cx.listener(move |editor, _, _, cx| {
                    editor.views.layout.selected = Some(writer.clone());
                    cx.notify();
                }),
            ),
        );
    }
    rows
}

/// The selected placement: where it is, what reads it, and the moves.
fn detail(editor: &Editor, one: &Placed, placed: &[Placed], found: &[Collision], cx: &mut Context<Editor>) -> Div {
    let mut column = div().flex().flex_col().flex_none().gap(px(GAP_SMALL)).child(strong(short(&one.writer)));
    for rect in &one.rects {
        column = column.child(mono(format!("VRAM x {} y {}, {}×{} words", rect.x, rect.y, rect.width, rect.height)));
    }
    column = column.child(mono(format!("Depth: {}", one.depth)));
    if let (Some(now), Some(disc)) = (one.load_arg, one.on_disc) {
        let moved = if now == disc { String::new() } else { format!(" (the disc stores {disc:#010x})") };
        column = column.child(mono(format!("Load argument: {now:#010x}{moved}")));
    }
    column = column.child(muted(one.note.clone()));
    for site in &one.sites {
        column = column.child(mono(format!("Read by: {site}")).text_color(color(MUTED)));
    }
    if one.kind != Kind::Code && !one.sites.is_empty() {
        column = column.child(muted(
            "These sites name the page and row as literals in code. A move changes the data only; patch them to follow it.",
        ));
    }
    let at = placed.iter().position(|other| other.writer == one.writer);
    for collision in found.iter().filter(|collision| Some(collision.first) == at || Some(collision.second) == at) {
        let other = &placed[if Some(collision.first) == at { collision.second } else { collision.first }];
        let area = collision.area;
        column = column.child(
            mono(format!(
                "Collides with {} at x {} y {}, {}×{} words",
                short(&other.writer),
                area.x,
                area.y,
                area.width,
                area.height
            ))
            .text_color(Hsla::from(rgba(COLLISION_FILL))),
        );
    }
    if one.kind == Kind::Code {
        return column;
    }
    let mut moves = div().flex().items_center().gap(px(GAP_SMALL)).child("Move");
    for (step, label) in Step::ALL {
        let writer = one.writer.clone();
        let allowed = editor.project.as_ref().is_some_and(|project| moved(project, one, step).is_ok());
        let mut control = button(("layout-move", step as usize), label, allowed);
        if allowed {
            control = control.on_click(cx.listener(move |editor, _, _, cx| {
                move_by(editor, &writer, step);
                cx.notify();
            }));
        }
        moves = moves.child(control);
    }
    let writer = one.writer.clone();
    moves = moves.child(separator(true)).child(button("layout-reset", "Back to the disc", one.moved()).on_click(
        cx.listener(move |editor, _, _, cx| {
            reset(editor, &writer);
            cx.notify();
        }),
    ));
    column.child(moves)
}

/// Moves the placement of `writer` one step.
fn move_by(editor: &mut Editor, writer: &Writer, step: Step) {
    let Some(project) = editor.project.as_mut() else { return };
    let Some(cache) = editor.views.layout.cache.clone() else { return };
    let Some(one) = cache.1.iter().find(|placed| placed.writer == *writer) else { return };
    let Writer::Entry { archive, slot } = writer else { return };
    let result = moved(project, one, step).and_then(|load_arg| {
        let loaded = project.load(&ArchivePath(archive.clone()))?;
        project.set_load_arg(&loaded, *slot, load_arg);
        Ok(load_arg)
    });
    editor.status = match result {
        Ok(load_arg) => format!("{} moved: load argument {load_arg:#010x}", short(writer)),
        Err(error) => error,
    };
}

/// Puts the placement of `writer` back to the disc's.
fn reset(editor: &mut Editor, writer: &Writer) {
    let (Some(project), Writer::Entry { archive, slot }) = (editor.project.as_mut(), writer) else { return };
    if let Ok(loaded) = project.load(&ArchivePath(archive.clone()))
        && let Some(disc) = loaded.entries().get(*slot).map(|entry| entry.ram_ptr)
    {
        project.set_load_arg(&loaded, *slot, disc);
        editor.status = format!("{} is back where the disc has it", short(writer));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette(writer: Writer, rects: Vec<Rect>) -> Placed {
        Placed {
            writer,
            kind: Kind::Palette,
            rects,
            depth: String::new(),
            load_arg: None,
            on_disc: None,
            note: String::new(),
            sites: Vec::new(),
        }
    }

    #[test]
    fn a_clut_block_spans_the_arena_rows_it_covers() {
        // 256 words from the arena start: one whole row at (0, 480).
        assert_eq!(palette_rects(0, 256), vec![Rect { x: 0, y: 480, width: 256, height: 1 }]);
        // 32 words from 240: the tail of row 480 and the head of row 481.
        assert_eq!(
            palette_rects(240, 32),
            vec![Rect { x: 240, y: 480, width: 16, height: 1 }, Rect { x: 0, y: 481, width: 16, height: 1 }]
        );
        // 600 words from word 16: head, one whole row, then the tail.
        let spans = palette_rects(16, 600);
        assert_eq!(spans.iter().map(|rect| usize::from(rect.width) * usize::from(rect.height)).sum::<usize>(), 600);
    }

    #[test]
    fn overlaps_are_collisions_and_code_is_left_out() {
        let at = |x, y, width, height| Rect { x, y, width, height };
        let one = palette(Writer::entry("A", 0), vec![at(0, 480, 64, 1)]);
        let two = palette(Writer::entry("A", 1), vec![at(32, 480, 64, 1)]);
        let apart = palette(Writer::entry("A", 2), vec![at(64, 481, 64, 1)]);
        let mut code = palette(Writer::Named("GFX".into()), vec![at(0, 480, 256, 32)]);
        code.kind = Kind::Code;
        let found = collisions(&[one, two, apart, code]);
        assert_eq!(found, vec![Collision { first: 0, second: 1, area: at(32, 480, 32, 1) }]);
    }

    /// AREA024's scene: its texture and block are placed from the type-3 entry and the arena, a move there and back
    /// leaves the project unedited, and the archive rebuilds as it was.
    #[test]
    fn a_retail_scene_lays_out_and_moves_back() {
        let Some(disc) = crate::retail_support::retail_disc() else { return };
        let folder = std::env::temp_dir().join(format!("bof3-edit-layout-{}", std::process::id()));
        let mut project = Project::open(disc.as_ref(), &folder).expect("project opens");
        let area = "BIN/WORLD00/AREA024.EMI";
        let scene = rebof3_emi::replay::scenes::home(area).expect("the area has a home scene");
        let replayed = super::super::scene::replay(&mut project, &scene);
        let placed = placements(&project, &replayed);

        let texture = placed.iter().find(|placed| placed.writer == Writer::entry(area, 7)).expect("entry 7");
        assert_eq!(texture.kind, Kind::Texture);
        let rect = texture.rects[0];
        assert!((rect.x..rect.x + rect.width).contains(&704) && (rect.y..rect.y + rect.height).contains(&256));
        assert!(texture.depth.contains("bpp"), "{}", texture.depth);
        let block = placed.iter().find(|placed| placed.writer == Writer::entry(area, 8)).expect("entry 8");
        assert_eq!(block.kind, Kind::Palette);
        assert!(block.rects.iter().any(|rect| rect.y >= 480));

        let loaded = project.load(&ArchivePath(area.into())).expect("loads");
        let before = project.current_archive(&loaded).expect("rebuilds");
        let right = moved(&project, texture, Step::Right).expect("a step right");
        project.set_load_arg(&loaded, 7, right);
        let again = super::super::scene::replay(&mut project, &scene);
        let now = placements(&project, &again);
        let shifted = now.iter().find(|placed| placed.writer == Writer::entry(area, 7)).expect("entry 7");
        assert_eq!(shifted.rects[0].x, texture.rects[0].x + 32);
        assert!(shifted.moved());
        let back = moved(&project, shifted, Step::Left).expect("a step left");
        project.set_load_arg(&loaded, 7, back);
        assert!(!project.is_dirty() || !project.is_edited(&(loaded.path.clone(), 7)));
        assert_eq!(project.current_archive(&loaded).expect("rebuilds"), before);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
