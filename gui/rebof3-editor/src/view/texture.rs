//! A texture upload as a paintable image.
//!
//! The upload's VRAM words read at 4, 8 or 16 bits per texel. A 4- or 8-bit texture is drawn with a
//! palette row. The view opens with the strongest evidence it has, in this order: the depth and row
//! the game itself drew the entry with on a route (`rebof3_observe`, cached per disc and route under
//! the project's `build/cache/observe/`), a row a rule joins to it in the scene that loads it
//! (`rebof3_emi::texture::scene`) or in its archive (`rebof3_formats::joins`), and otherwise 16 bits
//! with the reason shown. A suggested row of the scene (`rebof3_emi::texture::suggest`), a row of a
//! palette bank in the same archive and a grey ramp can be picked; a suggestion is listed, never
//! chosen for the user. A route runs in the background and can be cancelled. Painting writes texel
//! indices (or 16-bit colors) back into the words and the words back into the upload's rectangles;
//! Export TIM and Import TIM move the words through a standard TIM file.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::{
    AnyElement, Bounds, Context, Div, MouseButton, MouseDownEvent, MouseMoveEvent, Pixels, Point, anchored, canvas,
    deferred, div, fill, point, prelude::*, px, rgba, size,
};
use psx_formats::gfx::texture::{Clut, Depth, texel_indices, texel_words};
use psx_formats::gfx::vram;
use rebof3_emi::replay::Writer;
use rebof3_emi::replay::scenes;
use rebof3_emi::texture::{ResidentBank, clut_at, suggest};
use rebof3_formats::Format;
use rebof3_formats::clut::color_rgba;
use rebof3_formats::joins::joined_rows;
use rebof3_formats::vram::{self as texture_vram, TextureWords, texture_words};
use rebof3_observe::{Observation, Palette, Progress, Route, cache};

use super::palette::{SWATCH, SWATCH_GAP, words as words_of};
use super::{Entry, Menu, editing_menu};
use crate::app::{Editor, Menu as WindowMenu};
use crate::project::ArchivePath;
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
/// How often the view redraws a running route's progress.
const PROGRESS_TICK: Duration = Duration::from_millis(250);

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
    /// A palette row the game drew the texture with on a route.
    Observed(usize),
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
    /// The rows the routes drew the texture with, most-drawn first.
    observed: Vec<Row>,
    /// The most-drawn way the routes drew the texture, and its evidence.
    seen: Option<(Bits, String)>,
    joined: Vec<Row>,
    suggested: Vec<Row>,
    /// Why the texture opened at 16 bits, when nothing covers it.
    reason: Option<String>,
    observations: Observations,
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
            observed: Vec::new(),
            seen: None,
            joined: Vec::new(),
            suggested: Vec::new(),
            reason: None,
            observations: Observations::default(),
            depth_hint: None,
            hover: None,
            zoom_slider: Rc::default(),
            image: Rc::default(),
        }
    }
}

/// The cached observations of the open project's disc, and the route running for it.
#[derive(Default)]
struct Observations {
    /// The project folder the lookup started for; set during a draw, so it is a cell.
    looked_up: RefCell<Option<PathBuf>>,
    lookup: Lookup,
    /// Every fresh cached route of the disc.
    cached: Vec<Observation>,
    job: Option<Job>,
    /// How the last run ended.
    message: Option<String>,
}

/// Where the disc image and its cache key stand.
#[derive(Clone, Debug, Default)]
enum Lookup {
    #[default]
    Looking,
    Ready {
        image: PathBuf,
        sha: String,
    },
    Failed(String),
}

/// A route running in the background.
struct Job {
    route: String,
    frames: u64,
    frame: Arc<AtomicU64>,
    cancel: Arc<AtomicBool>,
}

/// What a finished run hands back.
type Finished = Arc<Mutex<Option<Result<Observation, String>>>>;

/// Picks the depth and colors the first time an upload is shown: the most-drawn way a cached route
/// saw the game draw it, else a joined row when the scene that loads it or its archive has one.
/// Otherwise the texture stays at 16 bits, the reason is shown and a suggestion is only listed.
pub fn prepare(editor: &mut Editor) {
    let Some(entry) = editor.current() else { return };
    let key = (entry.archive.path.0.clone(), entry.index);
    if editor.views.texture.key.as_ref() == Some(&key) {
        return;
    }
    let (archive, index) = key.clone();
    let (observed, seen) = match editor.project.as_mut() {
        Some(project) => {
            let mut words = |palette: &Palette, clut: Clut, depth: Depth| live_row(project, palette, clut, depth);
            observed_rows(&editor.views.texture.observations.cached, &archive, index, &mut words)
        }
        None => (Vec::new(), None),
    };
    let replayed = match (scenes::home(&archive), editor.project.as_mut()) {
        (Some(scene), Some(project)) => Some(super::scene::replay(project, &scene)),
        _ => None,
    };
    let Some(entry) = editor.current() else { return };
    let mut joined = Vec::new();
    let mut suggested = Vec::new();
    let mut depth_hint = None;
    let rules = match &replayed {
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
            format!("no rule of scene {} names it", replayed.scene.name)
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
            "no scene loads its archive and no archive rule names it".to_string()
        }
    };
    let state = &mut editor.views.texture;
    state.zoom = ZOOM_DEFAULT;
    state.ink = 0;
    state.hover = None;
    state.reason = None;
    match (&seen, joined.first()) {
        (Some((bits, _)), _) => {
            state.bits = *bits;
            state.source = if *bits == Bits::Sixteen { Source::Grey } else { Source::Observed(0) };
        }
        (None, Some(row)) => {
            state.bits = if row.depth == Depth::Four { Bits::Four } else { Bits::Eight };
            state.source = Source::Joined(0);
        }
        (None, None) => {
            state.bits = Bits::Sixteen;
            state.source = Source::Grey;
            state.reason = Some(format!("Shown at 16 bits: {}, and {rules}.", state.observations.status()));
        }
    }
    state.observed = observed;
    state.seen = seen;
    state.joined = joined;
    state.suggested = suggested;
    state.depth_hint = depth_hint;
    state.key = Some(key);
}

/// Reads the current words of a named palette's CLUT row.
type PaletteRow<'a> = dyn FnMut(&Palette, Clut, Depth) -> Option<Vec<u16>> + 'a;

/// The ways the cached routes drew entry `slot` of `archive`, most-drawn first: each indexed way as
/// a palette row in drawing order, and the most-drawn way with its evidence. `words` reads a named
/// palette's row from the project, so an edited bank shows its edit; a row nothing names keeps the
/// words the draw found.
fn observed_rows(
    cached: &[Observation],
    archive: &str,
    slot: usize,
    words: &mut PaletteRow<'_>,
) -> (Vec<Row>, Option<(Bits, String)>) {
    let mut uses: Vec<(&Observation, &rebof3_observe::Texture)> = cached
        .iter()
        .flat_map(|observation| observation.of(archive, slot).map(move |used| (observation, used)))
        .collect();
    uses.sort_by_key(|(_, used)| std::cmp::Reverse(used.draws));
    let mut rows = Vec::new();
    let mut seen = None;
    for (observation, used) in uses {
        let evidence = format!("{}; {} draws", used.evidence(&observation.route), used.draws);
        let (bits, row) = match (used.depth(), used.clut()) {
            (Some(depth), Some(clut)) => {
                let palette = match &used.palette {
                    Some(palette) => format!("{} #{}", palette.archive, palette.slot),
                    None => "no entry (the words the draw found)".to_string(),
                };
                let words =
                    used.palette.as_ref().and_then(|palette| words(palette, clut, depth)).unwrap_or(used.words.clone());
                let bits = if depth == Depth::Four { Bits::Four } else { Bits::Eight };
                let evidence = format!("{evidence}; CLUT {},{} written by {palette}", clut.x, clut.y);
                (bits, Some(Row { clut, depth, words, palette, evidence }))
            }
            _ => (Bits::Sixteen, None),
        };
        if seen.is_none() {
            let line = row.as_ref().map_or(format!("{evidence}; 15-bit, no CLUT"), |row| row.evidence.clone());
            seen = Some((bits, line));
        }
        rows.extend(row);
    }
    (rows, seen)
}

/// The current words of the CLUT row a named palette entry holds: an arena bank's row, or the words a
/// type-3 entry uploads at the row's place.
fn live_row(project: &mut crate::project::Project, palette: &Palette, clut: Clut, depth: Depth) -> Option<Vec<u16>> {
    let loaded = project.load(&ArchivePath(palette.archive.clone())).ok()?;
    let payload = project.payload(&loaded, palette.slot);
    let load_arg = project.load_arg(&loaded, palette.slot);
    match palette.via.as_str() {
        "arena" => ResidentBank::new(palette.archive.clone(), palette.slot, load_arg, payload)?
            .row(clut, depth)
            .map(<[u16]>::to_vec),
        _ => {
            let grid = texture_words(load_arg, payload).ok()?;
            let column = usize::from(clut.x).checked_sub(grid.x as usize)?;
            let row = usize::from(clut.y).checked_sub(grid.y as usize)?;
            let start = row * grid.width + column;
            (column + depth.colors() <= grid.width)
                .then(|| grid.words.get(start..start + depth.colors()))
                .flatten()
                .map(<[u16]>::to_vec)
        }
    }
}

impl Observations {
    /// Why no route covers the texture, as a clause.
    fn status(&self) -> String {
        match &self.lookup {
            Lookup::Looking => "the route cache is still being read".into(),
            Lookup::Failed(reason) => format!("no route can run ({reason})"),
            Lookup::Ready { .. } if self.cached.is_empty() => "no route has run for this disc".into(),
            Lookup::Ready { .. } => {
                let routes: Vec<&str> = self.cached.iter().map(|observation| observation.route.as_str()).collect();
                format!("route {} never drew it", routes.join(", "))
            }
        }
    }
}

/// Starts reading the project's route cache once per project folder: find the disc image, hash its
/// data track and read every fresh cache file, off the window's thread.
fn look_up(editor: &Editor, cx: &mut Context<Editor>) {
    let Some(project) = editor.project.as_ref() else { return };
    let state = &editor.views.texture.observations;
    if state.looked_up.borrow().as_ref() == Some(&project.folder) {
        return;
    }
    *state.looked_up.borrow_mut() = Some(project.folder.clone());
    let (disc, root) = (project.disc.clone(), project.folder.clone());
    let task = cx.background_executor().spawn(async move {
        let image = rebof3_observe::find_image(&disc)?;
        let sha = rebof3_observe::disc_sha(&image).map_err(|error| error.to_string())?;
        let cached = cache::load_all(&root, &sha);
        Ok::<_, String>((image, sha, cached))
    });
    let folder = project.folder.clone();
    cx.spawn(async move |this, cx| {
        let found = task.await;
        let _ = this.update(cx, |editor, cx| {
            if editor.project.as_ref().map(|project| &project.folder) != Some(&folder) {
                return;
            }
            let state = &mut editor.views.texture;
            match found {
                Ok((image, sha, cached)) => {
                    state.observations.lookup = Lookup::Ready { image, sha };
                    state.observations.cached = cached;
                }
                Err(reason) => state.observations.lookup = Lookup::Failed(reason),
            }
            state.key = None;
            cx.notify();
        });
    })
    .detach();
}

/// Runs `route` on the disc in a thread of its own, redraws its progress, and on success stores the
/// cache file and opens the texture again with what the route saw.
fn run_route(editor: &mut Editor, route: Route, cx: &mut Context<Editor>) {
    let Some(project) = editor.project.as_ref() else { return };
    let Lookup::Ready { image, sha } = editor.views.texture.observations.lookup.clone() else { return };
    let root = project.folder.clone();
    let frame = Arc::new(AtomicU64::new(0));
    let cancel = Arc::new(AtomicBool::new(false));
    let finished: Finished = Arc::default();
    let observations = &mut editor.views.texture.observations;
    observations.message = None;
    observations.job =
        Some(Job { route: route.name.clone(), frames: route.frames, frame: frame.clone(), cancel: cancel.clone() });

    let result = finished.clone();
    std::thread::spawn(move || {
        let run = rebof3_observe::run(&image, &route, &sha, |progress: Progress| {
            frame.store(progress.frame, Ordering::Relaxed);
            !cancel.load(Ordering::Relaxed)
        })
        .and_then(|observation| cache::store(&root, &observation).map(|_| observation))
        .map_err(|error| error.to_string());
        if let Ok(mut slot) = result.lock() {
            *slot = Some(run);
        }
    });

    cx.spawn(async move |this, cx| {
        loop {
            cx.background_executor().timer(PROGRESS_TICK).await;
            let done = finished.lock().ok().and_then(|mut slot| slot.take());
            let Ok(()) = this.update(cx, |editor, cx| {
                if let Some(run) = &done {
                    finish(editor, run.clone());
                }
                cx.notify();
            }) else {
                break;
            };
            if done.is_some() {
                break;
            }
        }
    })
    .detach();
}

/// Takes a finished run into the view: its observation replaces the route's old one.
fn finish(editor: &mut Editor, run: Result<Observation, String>) {
    let state = &mut editor.views.texture;
    let Some(job) = state.observations.job.take() else { return };
    match run {
        Ok(observation) => {
            state.observations.message = Some(format!(
                "Route {} ran {} frames in {:.1} s and saw {} entries drawn.",
                observation.route,
                observation.frames,
                observation.seconds,
                observation.coverage().values().sum::<usize>()
            ));
            state.observations.cached.retain(|old| old.route != observation.route);
            state.observations.cached.push(observation);
            state.key = None;
        }
        Err(error) => state.observations.message = Some(format!("Route {}: {error}.", job.route)),
    }
}

/// The colors a texture is drawn with.
fn palette(entry: &Entry<'_>, state: &State, depth: Depth) -> Vec<u16> {
    let colors = depth.colors();
    match &state.source {
        Source::Observed(at) => state.observed.get(*at).map(|row| row.words.clone()).unwrap_or_default(),
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
    look_up(editor, cx);
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
    let key = (entry.archive.path.clone(), entry.index);
    depth_row = depth_row
        .child(separator(true))
        .child(button("export-tim", "Export TIM", true).on_click(cx.listener(|editor, _, _, cx| {
            let Some(entry) = editor.current() else { return };
            let (name, bytes) = (tim_name(&entry), tim(&entry, &editor.views.texture));
            editor.save_file(name, bytes, cx);
        })))
        .child(button("import-tim", "Import TIM", true).on_click(cx.listener(move |editor, _, _, cx| {
            let key = key.clone();
            editor.open_file("TIM file to import", cx, move |editor, bytes| {
                import_tim(editor, &key, &bytes).unwrap_or_else(|error| error)
            });
        })));

    let mut column =
        div().flex().flex_col().flex_none().gap(px(GAP)).child(depth_row).child(routes(&state.observations, cx));
    if let Some(line) = evidence(state) {
        column = column.child(muted(line));
    }
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
    for (at, row) in state.observed.iter().enumerate().filter(|(_, row)| row.depth == depth) {
        choices.push((Source::Observed(at), label(row, "observed"), Some(row.evidence.clone())));
    }
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

/// The evidence for what the view shows: the route that drew it, the rule that joins it, or why it
/// is at 16 bits. A suggestion has its own line.
fn evidence(state: &State) -> Option<String> {
    let rule = |at: usize| state.joined.get(at).map(|row| format!("Rule: {}.", row.evidence));
    match &state.source {
        Source::Observed(at) => {
            let row = state.observed.get(*at)?;
            let mut line = format!("{}.", capitalized(&row.evidence));
            if let Some(joined) = state.joined.first()
                && (joined.depth != row.depth || joined.clut != row.clut)
            {
                line.push_str(&format!(" A rule names {} instead ({}).", label(joined, "a row"), joined.evidence));
            }
            Some(line)
        }
        Source::Joined(at) => rule(*at),
        Source::Grey if state.bits == Bits::Sixteen => match &state.seen {
            Some((Bits::Sixteen, line)) => Some(format!("{}.", capitalized(line))),
            _ => state.reason.clone(),
        },
        _ => None,
    }
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or(String::new(), |first| first.to_uppercase().chain(chars).collect())
}

/// The routes line: a button per built-in route, or the running route's progress and Cancel.
fn routes(observations: &Observations, cx: &mut Context<Editor>) -> Div {
    let mut line = div().flex().flex_wrap().items_center().gap(px(GAP)).child("Routes");
    if let Some(job) = &observations.job {
        let frame = job.frame.load(Ordering::Relaxed);
        let cancel = job.cancel.clone();
        return line.child(mono(format!("{}: frame {frame} of {}", job.route, job.frames))).child(
            button("cancel-route", "Cancel", true).on_click(cx.listener(move |_, _, _, cx| {
                cancel.store(true, Ordering::Relaxed);
                cx.notify();
            })),
        );
    }
    let ready = matches!(observations.lookup, Lookup::Ready { .. });
    for (at, name) in Route::built_in_names().enumerate() {
        let cached = observations.cached.iter().find(|observation| observation.route == name);
        let label = match cached {
            Some(observation) => {
                format!("Run {name} again ({} entries seen)", observation.coverage().values().sum::<usize>())
            }
            None => format!("Run {name}"),
        };
        line = line.child(button(("run-route", at), label, ready).on_click(cx.listener(move |editor, _, _, cx| {
            if editor.views.texture.observations.job.is_none()
                && let Some(route) = Route::built_in(name)
            {
                run_route(editor, route, cx);
            }
            cx.notify();
        })));
    }
    let status = match &observations.lookup {
        Lookup::Looking => Some("reading the route cache…".to_string()),
        Lookup::Failed(reason) => Some(reason.clone()),
        Lookup::Ready { .. } => observations.message.clone(),
    };
    if let Some(status) = status {
        line = line.child(muted(status));
    }
    line
}

/// The file name Export TIM offers: the archive's name and the slot.
fn tim_name(entry: &Entry<'_>) -> String {
    let (_, file) = entry.archive.path.split();
    format!("{}-{}.tim", file.trim_end_matches(".EMI").trim_end_matches(".emi"), entry.index)
}

/// The TIM of the texture as the view shows it: 15-bit with no CLUT at 16 bits, else with the shown
/// palette row at its VRAM place. The grey ramp and a bank row outside the palette arena have no
/// place, so they are refused instead of placed at a made-up one.
fn tim(entry: &Entry<'_>, state: &State) -> Result<Vec<u8>, String> {
    let Some(depth) = state.bits.depth() else {
        return texture_vram::texture_tim_direct(entry.load_arg(), entry.payload()).map_err(|error| error.to_string());
    };
    let clut = match &state.source {
        Source::Observed(at) => state.observed.get(*at).map(|row| row.clut),
        Source::Joined(at) => state.joined.get(*at).map(|row| row.clut),
        Source::Suggested(at) => state.suggested.get(*at).map(|row| row.clut),
        Source::Bank { bank, row } => {
            let load_arg = entry.project.load_arg(entry.archive, *bank);
            ResidentBank::new("", *bank, load_arg, entry.sibling(*bank))
                .map(|resident| clut_at(resident.bank.arena_word + row * depth.colors()))
        }
        Source::Grey => {
            return Err("The grey ramp is no palette row of the game; pick a row or export at 16 bits.".into());
        }
    }
    .ok_or("This palette row has no place in VRAM; pick another row or export at 16 bits.")?;
    let words = palette(entry, state, depth);
    texture_vram::texture_tim_of(entry.load_arg(), entry.payload(), depth.bits(), &words, clut.x, clut.y)
        .map_err(|error| error.to_string())
}

/// Reads a TIM into entry `key`: its image words become the payload and its image place the load
/// argument, as one undo step. The CLUT block stays in the file: the palette lives in its bank.
fn import_tim(editor: &mut Editor, key: &(ArchivePath, usize), bytes: &[u8]) -> Result<String, String> {
    let texture = texture_vram::import_tim(bytes).map_err(|error| error.to_string())?;
    let (archive, index) = key;
    let project = editor.project.as_mut().ok_or("no project is open")?;
    let loaded = project.load(archive)?;
    let current = project.load_arg(&loaded, *index);
    let load_arg = texture.placement.load_argument(current);
    editor.begin_step();
    let written = editor.rewrite_entry(archive, *index, |_| Ok(texture.payload.clone()));
    if written.is_ok()
        && load_arg != current
        && let Some(project) = editor.project.as_mut()
    {
        project.set_load_arg(&loaded, *index, load_arg);
    }
    editor.end_step();
    written?;
    editor.views.texture.key = None;
    let depth = texture.depth.map_or("15-bit".to_string(), |depth| format!("{}-bit", depth.bits()));
    let clut = if texture.clut.is_some() { "; its CLUT block is not imported, edit the palette bank" } else { "" };
    Ok(format!(
        "Imported a {depth} TIM: {}×{} words at ({}, {}){clut}.",
        texture.placement.chunks_per_row as usize * psx_formats::gfx::vram::RECTANGLE_SIDE,
        texture.height,
        texture.placement.x(),
        texture.placement.y()
    ))
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
        Source::Observed(at) => state.observed.get(*at).map_or("observed row".into(), |row| label(row, "observed")),
        Source::Joined(at) => state.joined.get(*at).map_or("joined row".into(), |row| label(row, "joined")),
        Source::Suggested(at) => state.suggested.get(*at).map_or("suggested row".into(), |row| label(row, "suggested")),
        Source::Bank { bank, row } => format!("bank {bank} row {row}"),
        Source::Grey => "grey ramp".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drawn(
        slot: usize,
        bits: u32,
        clut: Option<[u16; 2]>,
        draws: u64,
        palette: Option<Palette>,
    ) -> rebof3_observe::Texture {
        rebof3_observe::Texture {
            archive: "BIN/BATTLE/BATL_DRA.EMI".into(),
            slot,
            bits,
            clut,
            palette,
            words: vec![0x1234; if bits == 4 { 16 } else { 256 }],
            frame: 7000 + draws,
            opcode: 0x2C,
            draws,
            area: [896, 256, 960, 512],
        }
    }

    fn observation(textures: Vec<rebof3_observe::Texture>) -> Observation {
        Observation {
            version: rebof3_observe::FORMAT_VERSION,
            route: "new-game".into(),
            route_sha: String::new(),
            disc_sha: String::new(),
            player_revision: rebof3_observe::PLAYER_REVISION.into(),
            frames: 8100,
            seconds: 0.0,
            files: Vec::new(),
            uploads: 0,
            attributed_uploads: 0,
            textured_draws: 0,
            textures,
        }
    }

    #[test]
    fn the_most_drawn_way_opens_the_texture() {
        let bank = Palette { archive: "BIN/BATTLE/BATL_DRA.EMI".into(), slot: 1, via: "arena".into() };
        let cached = [observation(vec![
            drawn(0, 8, Some([0, 506]), 3, None),
            drawn(0, 4, Some([0, 507]), 40, Some(bank)),
            drawn(2, 4, Some([0, 506]), 99, None),
        ])];
        let mut live = |_: &Palette, _: Clut, depth: Depth| Some(vec![0x7FFF; depth.colors()]);
        let (rows, seen) = observed_rows(&cached, "BIN/BATTLE/BATL_DRA.EMI", 0, &mut live);
        assert_eq!(rows.len(), 2, "both indexed ways are rows; slot 2 is another entry");
        assert_eq!((rows[0].depth, rows[0].clut), (Depth::Four, Clut { x: 0, y: 507 }));
        assert_eq!(rows[0].words, vec![0x7FFF; 16], "a named bank's row is read from the project");
        assert_eq!(rows[1].words, vec![0x1234; 256], "an unnamed row keeps the words the draw found");
        let (bits, evidence) = seen.expect("the entry was drawn");
        assert_eq!(bits, Bits::Four);
        assert!(evidence.starts_with("observed: route new-game, frame 7040, GP0(2Ch); 40 draws"), "{evidence}");
        assert!(evidence.contains("BIN/BATTLE/BATL_DRA.EMI #1"), "{evidence}");

        let (rows, seen) = observed_rows(&cached, "BIN/BATTLE/BATL_DRA.EMI", 3, &mut live);
        assert!(rows.is_empty() && seen.is_none(), "an entry no route drew keeps the rules and the suggestion");
    }

    #[test]
    fn the_grey_ramp_runs_from_black_to_white() {
        assert_eq!(grey(0, 16), 0);
        assert_eq!(grey(15, 16), 0x7FFF);
    }
}
