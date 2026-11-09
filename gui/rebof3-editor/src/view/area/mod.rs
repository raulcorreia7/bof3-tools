//! An area in 3D, as the field renderer draws it, with the field data laid over it and edited in place.

mod model;

use std::cell::{Cell as Shared, RefCell};
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    AnyElement, Bounds, Context, Corners, Div, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, RenderImage, ScrollWheelEvent, canvas, div, prelude::*, px,
};
use rebof3_emi::replay::scenes;
use rebof3_emi::text::codec::name_text;
use rebof3_formats::encounter::ENCOUNTER_ENEMIES;
use rebof3_formats::field::{FACINGS, GAME_CODE};
use rebof3_formats::map::CELL_BYTES;
use rebof3_formats::map::draw::{Face, TileField};
use rebof3_render::{Backend, Camera, Renderer, Scene};

use self::model::{Area, Hit, Layers, Sources, View};
use super::scene::Replayed;
use super::{Entry, Field, input};
use crate::app::Editor;
use crate::project::{ArchivePath, Project};
use crate::schema;
use crate::theme::{MUTED, color};
use crate::widgets::{GAP, GAP_SMALL, button, checkbox, mono, muted, separator, strong, tab};

/// The executable the field tables are read from, in the disc folder.
const EXECUTABLE: &str = "SLUS_004.22";
/// The width of the side panel and of a number field, in pixels.
const PANEL_WIDTH: f32 = 340.0;
const NUMBER_WIDTH: f32 = 56.0;
const WIDE_WIDTH: f32 = 96.0;
/// The frame drawn before the view knows its size, and the largest it draws.
const FIRST_FRAME: (usize, usize) = (640, 480);
const LARGEST_FRAME: usize = 1600;
/// How far the pointer moves before a press becomes a drag, in pixels.
const DRAG_SLOP: f32 = 3.0;
/// Radians of turn per pixel of drag, the zoom per wheel pixel, and the pitch the free camera keeps within.
const TURN_PER_PIXEL: f32 = 0.01;
const ZOOM_PER_PIXEL: f32 = 0.004;
const PITCH_LIMIT: f32 = 1.5;
/// The nearest and farthest the free camera orbits, in renderer units.
const NEAREST: f32 = 64.0;
const FARTHEST: f32 = 40_000.0;
/// Half cells the game camera moves per pixel of a pan.
const HALF_CELLS_PER_PIXEL: f32 = 0.1;

/// What a press on the view does.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Tool {
    /// Picks what is under the pointer; a drag turns the free camera.
    #[default]
    Select,
    /// Writes the code ink to each cell the drag passes.
    PaintCode,
    /// Writes the battle ink to each cell the drag passes.
    PaintBattle,
    /// Raises or lowers each cell the drag passes by one height step.
    Raise,
    Lower,
    /// Drags an actor from cell to cell.
    Move,
}

impl Tool {
    const ALL: [Tool; 6] = [Tool::Select, Tool::PaintCode, Tool::PaintBattle, Tool::Raise, Tool::Lower, Tool::Move];

    fn label(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::PaintCode => "Paint code",
            Tool::PaintBattle => "Paint battle",
            Tool::Raise => "Raise",
            Tool::Lower => "Lower",
            Tool::Move => "Move actor",
        }
    }
}

/// A value of the area view a field edits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Part {
    /// A corner height, the byte code or the battle value of the picked cell.
    Corner(usize),
    Code,
    Battle,
    /// A field of the picked face's descriptor.
    Tile(TileField),
    /// The picked actor's position in cells (`3.5` is half a cell on), facing and scene id.
    ActorX,
    ActorY,
    Facing,
    Scene,
    /// An enemy slot (`0` to `7`) or the rate (`8`) of an encounter row.
    Encounter {
        row: usize,
        slot: usize,
    },
    /// What the paint tools write.
    CodeInk,
    BattleInk,
}

/// What is picked: a face of a cell, or an actor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Target {
    Face { cell: usize, face: Face },
    Actor(usize),
}

/// A press in progress.
#[derive(Clone, Copy, Debug)]
struct Drag {
    button: MouseButton,
    start: Point<Pixels>,
    last: Point<Pixels>,
    moved: bool,
    /// The cell the drag last edited, so a stroke edits each cell once.
    cell: Option<usize>,
    /// The actor a move drags.
    actor: Option<usize>,
    /// Whether the drag opened an undo step.
    step: bool,
}

/// The area the view read, and what it was read from.
struct Read {
    key: (String, usize, u64),
    area: Result<Rc<Area>, String>,
}

/// The frame on screen and what it was drawn from.
struct Drawn {
    key: DrawKey,
    image: Arc<RenderImage>,
    scene: Rc<Scene>,
    camera: Camera,
    size: (usize, usize),
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct DrawKey {
    revision: u64,
    view: View,
    layers: Layers,
    picked: Option<Hit>,
    size: (usize, usize),
    software: bool,
}

pub struct State {
    /// The archive and entry the camera and pick belong to.
    owner: Option<(String, usize)>,
    view: Option<View>,
    layers: Layers,
    tool: Tool,
    code_ink: u8,
    battle_ink: u8,
    picked: Option<Target>,
    hover: Option<Hit>,
    drag: Option<Drag>,
    /// Draw with the software path even when OpenGL ES is there.
    software: bool,
    renderer: Option<Renderer>,
    read: Option<Read>,
    drawn: Option<Drawn>,
    /// Frames no longer shown, which the next paint drops from the window's atlas.
    retired: Rc<RefCell<Vec<Arc<RenderImage>>>>,
    bounds: Rc<Shared<Bounds<Pixels>>>,
    sources: Cache,
}

impl Default for State {
    fn default() -> Self {
        Self {
            owner: None,
            view: None,
            layers: Layers::default(),
            tool: Tool::Select,
            code_ink: 0,
            battle_ink: 0,
            picked: None,
            hover: None,
            drag: None,
            software: false,
            renderer: None,
            read: None,
            drawn: None,
            retired: Rc::default(),
            bounds: Rc::default(),
            sources: Cache::default(),
        }
    }
}

/// What the view reads once and keeps: the executable, and the last scene replay with the revision it holds for.
#[derive(Default)]
pub struct Cache {
    exe: Option<Option<Rc<Vec<u8>>>>,
    replay: Option<(Rc<Replayed>, u64)>,
}

impl State {
    /// What the shown area read, for the tests.
    #[cfg(test)]
    pub fn summary(&self) -> Option<String> {
        let read = self.read.as_ref()?;
        Some(match &read.area {
            Ok(area) => summary(area),
            Err(error) => error.clone(),
        })
    }

    /// The backend that drew the shown frame.
    #[cfg(test)]
    pub fn drawn(&self) -> Option<(usize, usize)> {
        self.drawn.as_ref().map(|drawn| drawn.size)
    }
}

/// Reads area entry `index` of `archive` with every source beside it.
pub fn read_area(
    project: &mut Project,
    archive: &ArchivePath,
    index: usize,
    cache: &mut Cache,
) -> Result<Area, String> {
    let scene = scenes::home(&archive.0);
    let replayed = match (&scene, &cache.replay) {
        (Some(scene), Some((replayed, revision))) if replayed.scene == *scene && *revision == project.revision() => {
            Some(Rc::clone(replayed))
        }
        (Some(scene), _) => Some(super::scene::replay(project, scene)),
        (None, _) => None,
    };
    if let Some(replayed) = &replayed {
        cache.replay = Some((Rc::clone(replayed), project.revision()));
    }
    let exe = cache.exe.get_or_insert_with(|| read_exe(&project.disc)).clone();
    let game = project.load(&ArchivePath(schema::GAME.into())).ok();
    let loaded = project.load(archive)?;

    let project = &*project;
    let entries: Vec<(u32, &[u8])> = (0..loaded.entries().len())
        .map(|entry| (project.load_arg(&loaded, entry), project.payload(&loaded, entry)))
        .collect();
    let game_code = game.as_ref().and_then(|game| {
        (0..game.entries().len())
            .find(|&entry| project.load_arg(game, entry) == GAME_CODE)
            .map(|entry| project.payload(game, entry))
    });
    let bundle = entries.get(index).ok_or("no such entry")?.1;
    let sources = Sources {
        bundle,
        entries: entries.clone(),
        area: scenes::area_id(&archive.0).map(usize::from),
        exe: exe.as_deref().map(Vec::as_slice),
        game_code,
        vram: replayed.as_ref().map(|replayed| (&replayed.vram, replayed.scene.name.as_str())),
    };
    Area::read(&sources)
}

fn read_exe(disc: &Path) -> Option<Rc<Vec<u8>>> {
    std::fs::read(disc.join(EXECUTABLE)).ok().map(Rc::new)
}

/// Reads the shown area once per entry and revision, and draws it once per change of what is shown.
pub fn prepare(editor: &mut Editor) {
    if let Some(key) = refresh(editor) {
        draw(editor, &key);
    }
}

/// Reads the shown area again when its entry or the project changed, and returns what it read.
fn refresh(editor: &mut Editor) -> Option<(String, usize, u64)> {
    let entry = editor.current()?;
    let owner = (entry.archive.path.0.clone(), entry.index);
    let key = (owner.0.clone(), owner.1, entry.project.revision());
    let archive = entry.archive.path.clone();
    let state = &mut editor.views.area;
    if state.owner.as_ref() != Some(&owner) {
        state.owner = Some(owner.clone());
        state.view = None;
        state.picked = None;
        state.hover = None;
        state.drag = None;
    }

    if state.read.as_ref().is_none_or(|read| read.key != key) {
        let project = editor.project.as_mut()?;
        let area = read_area(project, &archive, owner.1, &mut editor.views.area.sources).map(Rc::new);
        editor.views.area.read = Some(Read { key: key.clone(), area });
    }
    Some(key)
}

/// Draws the read area, unless the frame on screen shows the same thing.
fn draw(editor: &mut Editor, key: &(String, usize, u64)) {
    let state = &mut editor.views.area;
    let Some(Read { area: Ok(area), .. }) = &state.read else {
        state.drawn = None;
        return;
    };
    let area = Rc::clone(area);
    let view = *state.view.get_or_insert_with(|| View::overview(&area));
    let bounds = state.bounds.get();
    let size = frame_size(f32::from(bounds.size.width), f32::from(bounds.size.height));
    let picked = state.picked.and_then(|target| hit_of(&area, target));
    let draw = DrawKey { revision: key.2, view, layers: state.layers, picked, size, software: state.software };
    if state.drawn.as_ref().is_some_and(|drawn| drawn.key == draw) {
        return;
    }

    let camera = view.camera(&area);
    let scene = model::scene(&area, state.layers, &camera, picked);
    if state.renderer.as_ref().is_none_or(|renderer| (renderer.backend() == Backend::Gles) == state.software) {
        state.renderer = Some(Renderer::new(!state.software));
    }
    let Some(renderer) = state.renderer.as_mut() else { return };
    let frame = renderer.draw(&scene, &camera, size.0, size.1);
    let Some(image) = render_image(frame) else { return };
    if let Some(old) = state.drawn.take() {
        state.retired.borrow_mut().push(old.image);
    }
    state.drawn = Some(Drawn { key: draw, image, scene: Rc::new(scene), camera, size });
}

/// The frame size for a view of `width` by `height` pixels: the view's own size, no larger than [`LARGEST_FRAME`] on
/// its longer side.
fn frame_size(width: f32, height: f32) -> (usize, usize) {
    if width < 1.0 || height < 1.0 {
        return FIRST_FRAME;
    }
    let scale = (LARGEST_FRAME as f32 / width.max(height)).min(1.0);
    (((width * scale) as usize).max(1), ((height * scale) as usize).max(1))
}

/// A frame as an image gpui paints: BGRA rows.
fn render_image(frame: rebof3_render::Frame) -> Option<Arc<RenderImage>> {
    let mut bytes = frame.rgba;
    for pixel in bytes.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    let buffer = image::RgbaImage::from_raw(frame.width as u32, frame.height as u32, bytes)?;
    Some(Arc::new(RenderImage::new([image::Frame::new(buffer)])))
}

/// The hit of a target in the read area.
fn hit_of(area: &Area, target: Target) -> Option<Hit> {
    match target {
        Target::Face { cell, face } => {
            area.quads.iter().position(|quad| quad.cell == cell && quad.face == face).map(Hit::Quad)
        }
        Target::Actor(index) => (index < area.actor_list().len()).then_some(Hit::Actor(index)),
    }
}

/// The target of a hit.
fn target_of(area: &Area, hit: Hit) -> Option<Target> {
    match hit {
        Hit::Quad(index) => area.quads.get(index).map(|quad| Target::Face { cell: quad.cell, face: quad.face }),
        Hit::Actor(index) => Some(Target::Actor(index)),
    }
}

/// The area of the shown entry, read again first when an edit made it stale.
fn shown_area(editor: &mut Editor) -> Option<Rc<Area>> {
    refresh(editor)?;
    editor.views.area.read.as_ref()?.area.as_ref().ok().cloned()
}

/// Keeps the scene replay current across an edit of this view: the view writes the bundle, the battle grid, the area
/// program and the encounter block, and the replay reads none of them for the VRAM.
fn keep_replay(editor: &mut Editor, before: u64) {
    let after = editor.project.as_ref().map_or(before, Project::revision);
    if let Some((_, revision)) = editor.views.area.sources.replay.as_mut()
        && *revision == before
    {
        *revision = after;
    }
}

fn revision(editor: &Editor) -> u64 {
    editor.project.as_ref().map_or(0, Project::revision)
}

/// What is under a window position.
fn hit_at(editor: &Editor, position: Point<Pixels>) -> Option<Hit> {
    let drawn = editor.views.area.drawn.as_ref()?;
    let bounds = editor.views.area.bounds.get();
    let (width, height) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    if width < 1.0 || height < 1.0 {
        return None;
    }
    let x = f32::from(position.x - bounds.origin.x) / width * drawn.size.0 as f32;
    let y = f32::from(position.y - bounds.origin.y) / height * drawn.size.1 as f32;
    let id = rebof3_render::pick(&drawn.scene, &drawn.camera, drawn.size.0, drawn.size.1, x, y)?;
    Hit::of_id(id)
}

/// A one-line account of what the area read.
fn summary(area: &Area) -> String {
    let actors = match &area.actors {
        Ok(actors) => format!("{} actors", actors.len()),
        Err(error) => format!("actors unknown ({error})"),
    };
    let zones = match &area.zones {
        Ok(zones) => format!("{} zone rows", zones.len()),
        Err(error) => format!("zones unknown ({error})"),
    };
    let textured = match &area.scene_name {
        Some(name) => {
            format!("textured from {name}, {} quads whose page or CLUT row it does not write", area.unwritten)
        }
        None => "untextured: no scene loads this archive".into(),
    };
    format!(
        "{} x {} cells, {} quads, {textured}; {actors}; {zones}; battle grid {}; encounters {}.",
        area.width(),
        area.height(),
        area.quads.len(),
        if area.battle.is_some() { "read" } else { "absent" },
        if area.encounter_block().is_some() { "read" } else { "absent" },
    )
}

pub fn show(editor: &Editor, _entry: &Entry<'_>, cx: &mut Context<Editor>) -> AnyElement {
    let state = &editor.views.area;
    let area = match state.read.as_ref().map(|read| &read.area) {
        Some(Ok(area)) => Rc::clone(area),
        Some(Err(error)) => return muted(format!("The area does not read: {error}")).into_any_element(),
        None => return muted("Reading the area.").into_any_element(),
    };

    let hover = match state.hover {
        Some(hit) => model::describe(&area, hit).join(" "),
        None => "Point at a cell or an actor to read it. Drag to turn, right-drag to pan, scroll to zoom.".into(),
    };
    let body = div().flex().flex_1().min_h_0().gap(px(GAP)).child(viewport(editor, cx)).child(panel(editor, &area, cx));
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .gap(px(GAP))
        .child(muted(summary(&area)))
        .child(controls(editor, cx))
        .child(separator(false))
        .child(mono(hover).text_color(color(MUTED)))
        .child(body)
        .into_any_element()
}

/// The drawn frame, with the pointer's gestures.
fn viewport(editor: &Editor, cx: &mut Context<Editor>) -> impl IntoElement {
    let state = &editor.views.area;
    let measure = Rc::clone(&state.bounds);
    let retired = Rc::clone(&state.retired);
    let image = state.drawn.as_ref().map(|drawn| Arc::clone(&drawn.image));
    let frame = canvas(
        move |bounds, window, _| {
            if measure.get() != bounds {
                measure.set(bounds);
                window.refresh();
            }
        },
        move |bounds, (), window, _| {
            for old in retired.borrow_mut().drain(..) {
                let _ = window.drop_image(old);
            }
            if let Some(image) = image {
                let _ = window.paint_image(bounds, Corners::default(), image, 0, false);
            }
        },
    )
    .size_full();

    div()
        .id("area-view")
        .flex_1()
        .min_w_0()
        .min_h_0()
        .child(frame)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|editor, event: &MouseDownEvent, _, cx| {
                press(editor, MouseButton::Left, event.position);
                cx.notify();
            }),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(|editor, event: &MouseDownEvent, _, cx| {
                press(editor, MouseButton::Right, event.position);
                cx.notify();
            }),
        )
        .on_mouse_move(cx.listener(|editor, event: &MouseMoveEvent, _, cx| {
            moved(editor, event);
            cx.notify();
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|editor, event: &MouseUpEvent, _, cx| {
                release(editor, event.position);
                cx.notify();
            }),
        )
        .on_mouse_up(
            MouseButton::Right,
            cx.listener(|editor, event: &MouseUpEvent, _, cx| {
                release(editor, event.position);
                cx.notify();
            }),
        )
        .on_scroll_wheel(cx.listener(|editor, event: &ScrollWheelEvent, _, cx| {
            let delta = f32::from(event.delta.pixel_delta(px(16.0)).y);
            if let Some(View::Orbit { distance, .. }) = editor.views.area.view.as_mut() {
                *distance = (*distance * (1.0 - delta * ZOOM_PER_PIXEL)).clamp(NEAREST, FARTHEST);
            }
            cx.notify();
        }))
}

fn press(editor: &mut Editor, button: MouseButton, position: Point<Pixels>) {
    editor.stop_editing();
    let hit = hit_at(editor, position);
    let mut drag = Drag { button, start: position, last: position, moved: false, cell: None, actor: None, step: false };
    if button == MouseButton::Left {
        match editor.views.area.tool {
            Tool::Select => {}
            Tool::Move => {
                if let Some(Hit::Actor(index)) = hit {
                    editor.views.area.picked = Some(Target::Actor(index));
                    drag.actor = Some(index);
                    drag.step = true;
                    editor.begin_step();
                }
            }
            _ => {
                drag.step = true;
                editor.begin_step();
                editor.views.area.drag = Some(drag);
                stroke(editor, hit);
                return;
            }
        }
    }
    editor.views.area.drag = Some(drag);
}

fn moved(editor: &mut Editor, event: &MouseMoveEvent) {
    let hit = hit_at(editor, event.position);
    editor.views.area.hover = hit;
    let Some(mut drag) = editor.views.area.drag else { return };
    if event.pressed_button != Some(drag.button) {
        finish(editor);
        return;
    }
    let (dx, dy) = (f32::from(event.position.x - drag.last.x), f32::from(event.position.y - drag.last.y));
    let travelled = event.position - drag.start;
    drag.moved |= f32::from(travelled.x).abs() + f32::from(travelled.y).abs() > DRAG_SLOP;
    drag.last = event.position;
    editor.views.area.drag = Some(drag);
    if !drag.moved {
        return;
    }
    match (drag.button, editor.views.area.tool) {
        (MouseButton::Left, Tool::Select) => turn(editor, dx, dy),
        (MouseButton::Left, Tool::Move) => {
            if drag.actor.is_some() {
                stroke(editor, hit);
            } else {
                turn(editor, dx, dy);
            }
        }
        (MouseButton::Left, _) => stroke(editor, hit),
        _ => pan(editor, dx, dy),
    }
}

fn release(editor: &mut Editor, position: Point<Pixels>) {
    let Some(drag) = editor.views.area.drag else { return };
    if drag.button == MouseButton::Left && !drag.moved && editor.views.area.tool == Tool::Select {
        let area = shown_area(editor);
        editor.views.area.picked = hit_at(editor, position).zip(area).and_then(|(hit, area)| target_of(&area, hit));
    }
    finish(editor);
}

fn finish(editor: &mut Editor) {
    if editor.views.area.drag.take().is_some_and(|drag| drag.step) {
        editor.end_step();
    }
}

/// Turns the free camera.
fn turn(editor: &mut Editor, dx: f32, dy: f32) {
    if let Some(View::Orbit { yaw, pitch, .. }) = editor.views.area.view.as_mut() {
        *yaw -= dx * TURN_PER_PIXEL;
        *pitch = (*pitch + dy * TURN_PER_PIXEL).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    }
}

/// Moves what the camera looks at, along the ground.
fn pan(editor: &mut Editor, dx: f32, dy: f32) {
    match editor.views.area.view.as_mut() {
        Some(View::Orbit { target, yaw, distance, .. }) => {
            let scale = *distance / 600.0;
            let (sin, cos) = yaw.sin_cos();
            target[0] -= (dx * cos - dy * sin) * scale;
            target[2] -= (-dx * sin - dy * cos) * scale;
        }
        Some(View::Game { x2, y2 }) => {
            let step = |value: &mut u16, by: f32| {
                *value = (f32::from(*value) - by * HALF_CELLS_PER_PIXEL).clamp(0.0, f32::from(u16::MAX)) as u16;
            };
            step(x2, dx);
            step(y2, dy);
        }
        None => {}
    }
}

/// Applies the tool to the cell under the pointer, once per cell a stroke passes.
fn stroke(editor: &mut Editor, hit: Option<Hit>) {
    let Some(area) = shown_area(editor) else { return };
    let Some(cell) = hit.and_then(|hit| match hit {
        Hit::Quad(index) => area.quads.get(index).map(|quad| quad.cell),
        Hit::Actor(_) => None,
    }) else {
        return;
    };
    let Some(mut drag) = editor.views.area.drag else { return };
    if drag.cell == Some(cell) {
        return;
    }
    drag.cell = Some(cell);
    editor.views.area.drag = Some(drag);
    let state = &editor.views.area;
    let result = match state.tool {
        Tool::PaintCode => {
            let ink = state.code_ink;
            rewrite_bundle(editor, |payload| model::with_code(payload, cell, ink))
        }
        Tool::PaintBattle => {
            let ink = state.battle_ink;
            rewrite_sibling(editor, area.battle.as_ref().map(|(index, _)| *index), "battle grid", |grid| {
                model::with_battle(grid, cell, ink)
            })
        }
        Tool::Raise => rewrite_bundle(editor, |payload| model::with_raised(payload, cell, 1)),
        Tool::Lower => rewrite_bundle(editor, |payload| model::with_raised(payload, cell, -1)),
        Tool::Move => match drag.actor {
            Some(actor) => {
                let (column, row) = area.column_row(cell);
                let (x2, y2) = ((2 * column) as u16, (2 * row) as u16);
                rewrite_program(editor, &area, |area| model::with_actor_position(area, actor, x2, y2))
            }
            None => Ok(()),
        },
        Tool::Select => Ok(()),
    };
    if let Err(error) = result {
        editor.status = error;
    }
}

/// Rewrites the shown bundle.
fn rewrite_bundle(editor: &mut Editor, edit: impl FnOnce(&[u8]) -> Result<Vec<u8>, String>) -> Result<(), String> {
    let index = editor.entry.ok_or("no entry is shown")?;
    rewrite_sibling(editor, Some(index), "map bundle", edit)
}

/// Rewrites entry `index` of the shown archive.
fn rewrite_sibling(
    editor: &mut Editor,
    index: Option<usize>,
    what: &str,
    edit: impl FnOnce(&[u8]) -> Result<Vec<u8>, String>,
) -> Result<(), String> {
    let index = index.ok_or_else(|| format!("the area has no {what}"))?;
    let archive = editor.archive.as_ref().ok_or("no archive is open")?.path.clone();
    let before = revision(editor);
    editor.rewrite_entry(&archive, index, edit)?;
    keep_replay(editor, before);
    Ok(())
}

/// Rewrites the area program with an edit computed from the read area.
fn rewrite_program(
    editor: &mut Editor,
    area: &Area,
    edit: impl FnOnce(&Area) -> Result<Vec<u8>, String>,
) -> Result<(), String> {
    let index = area.program.as_ref().map(|(index, _)| *index);
    let payload = edit(area)?;
    rewrite_sibling(editor, index, "area program", |_| Ok(payload))
}

/// The switch of one layer in [`Layers`].
type LayerFlag = fn(&mut Layers) -> &mut bool;

/// The camera, layer, tool and renderer controls.
fn controls(editor: &Editor, cx: &mut Context<Editor>) -> Div {
    let state = &editor.views.area;
    let orbit = matches!(state.view, Some(View::Orbit { .. }) | None);
    let mut row = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(GAP_SMALL))
        .child(tab("area-orbit", "Free camera", orbit).on_click(cx.listener(|editor, _, _, cx| {
            if let Some(area) = shown_area(editor) {
                editor.views.area.view = Some(View::overview(&area));
            }
            cx.notify();
        })))
        .child(tab("area-game", "Game camera", !orbit).on_click(cx.listener(|editor, _, _, cx| {
            if let Some(area) = shown_area(editor) {
                editor.views.area.view = Some(follow(&area, editor.views.area.picked));
            }
            cx.notify();
        })))
        .child(separator(true));

    let layers: [(&str, &str, LayerFlag); 6] = [
        ("area-terrain", "Tiles", |layers| &mut layers.terrain),
        ("area-exits", "Exits", |layers| &mut layers.exits),
        ("area-codes", "Codes", |layers| &mut layers.codes),
        ("area-battle", "Battle grid", |layers| &mut layers.battle),
        ("area-zones", "Zones", |layers| &mut layers.zones),
        ("area-actors", "Actors", |layers| &mut layers.actors),
    ];
    for (id, label, flag) in layers {
        let mut shown = state.layers;
        let checked = *flag(&mut shown);
        row = row.child(checkbox(id, label, checked).on_click(cx.listener(move |editor, _, _, cx| {
            let value = flag(&mut editor.views.area.layers);
            *value = !*value;
            cx.notify();
        })));
    }
    row = row.child(separator(true));
    for tool in Tool::ALL {
        row = row.child(tab(tool.label(), tool.label(), state.tool == tool).on_click(cx.listener(
            move |editor, _, _, cx| {
                editor.views.area.tool = tool;
                cx.notify();
            },
        )));
    }
    row = row
        .child(muted("Code ink"))
        .child(input(
            editor,
            "area-code-ink",
            Field::Area(Part::CodeInk),
            &format!("{:#04X}", state.code_ink),
            "",
            Some(NUMBER_WIDTH),
            cx,
        ))
        .child(muted("Battle ink"))
        .child(input(
            editor,
            "area-battle-ink",
            Field::Area(Part::BattleInk),
            &state.battle_ink.to_string(),
            "",
            Some(NUMBER_WIDTH),
            cx,
        ))
        .child(separator(true));

    let backend = match state.renderer.as_ref().map(Renderer::backend) {
        Some(Backend::Gles) => "OpenGL ES".to_string(),
        Some(Backend::Software) => match state.renderer.as_ref().and_then(|renderer| renderer.fallback.clone()) {
            Some(reason) if !state.software => format!("software ({reason})"),
            _ => "software".into(),
        },
        None => "none yet".into(),
    };
    row.child(checkbox("area-software", "Software renderer", state.software).on_click(cx.listener(
        |editor, _, _, cx| {
            editor.views.area.software = !editor.views.area.software;
            cx.notify();
        },
    )))
    .child(muted(format!("Drawn by {backend}.")))
    .child(separator(true))
    .child(button("area-export", "Export glTF", true).on_click(cx.listener(|editor, _, _, cx| {
        let Some(area) = shown_area(editor) else { return };
        let Some(entry) = editor.current() else { return };
        let name = format!("{}-{}", entry.archive.path.0.replace('/', "_"), entry.index);
        let glb = model::export(&area, &name).ok_or_else(|| "the area has no drawn cell".to_string());
        editor.save_file(format!("{name}.glb"), glb, cx);
    })))
    .child(button("area-import", "Import glTF", true).on_click(cx.listener(|editor, _, _, cx| {
        editor.open_file("glTF of this area to import", cx, |editor, glb| match import(editor, &glb) {
            Ok(message) => message,
            Err(error) => error,
        });
    })))
}

/// Writes back what a glTF carries: the corner heights and the actors' places, as one undo step.
fn import(editor: &mut Editor, glb: &[u8]) -> Result<String, String> {
    let area = shown_area(editor).ok_or("the area is not read")?;
    let bundle_index = editor.entry.ok_or("no entry is shown")?;
    let archive = editor.archive.as_ref().ok_or("no archive is open")?.path.clone();
    let bundle = editor.current().ok_or("no entry is shown")?.payload().to_vec();
    let (rebuilt, program) = model::import(&area, &bundle, glb)?;
    let before = revision(editor);
    let written = match (program, area.program.as_ref().map(|(index, _)| *index)) {
        (Some(program), Some(program_index)) => {
            editor.rewrite_entries(&archive, &[bundle_index, program_index], |_| Ok(vec![rebuilt, program]))?;
            Ok("Imported the heights and the actors' places.".into())
        }
        _ => {
            editor.rewrite_entry(&archive, bundle_index, |_| Ok(rebuilt))?;
            Ok("Imported the heights; no actor moved.".into())
        }
    };
    keep_replay(editor, before);
    written
}

/// The game camera on the picked actor or cell, else on the map's centre.
fn follow(area: &Area, picked: Option<Target>) -> View {
    match picked {
        Some(Target::Actor(index)) if index < area.actor_list().len() => {
            let placement = area.actor_list()[index].placement;
            View::Game { x2: placement.x2, y2: placement.y2 }
        }
        Some(Target::Face { cell, .. }) => {
            let (column, row) = area.column_row(cell);
            View::Game { x2: (2 * column) as u16, y2: (2 * row) as u16 }
        }
        _ => View::game(area),
    }
}

/// The picked record's fields and the area's encounter rows.
fn panel(editor: &Editor, area: &Area, cx: &mut Context<Editor>) -> impl IntoElement {
    let mut panel =
        div().id("area-panel").flex().flex_col().flex_none().w(px(PANEL_WIDTH)).gap(px(GAP_SMALL)).overflow_y_scroll();
    panel = match editor.views.area.picked {
        Some(Target::Face { cell, face }) if cell < area.bundle.cells.len() => {
            panel.child(cell_fields(editor, area, cell, face, cx))
        }
        Some(Target::Actor(index)) if index < area.actor_list().len() => {
            panel.child(actor_fields(editor, area, index, cx))
        }
        _ => panel.child(muted("Nothing picked. Click a cell or an actor.")),
    };
    panel
        .child(separator(false))
        .child(encounter_fields(editor, area, cx))
        .child(separator(false))
        .child(zone_list(area))
}

/// A label and a field on one line.
fn labelled(label: impl Into<gpui::SharedString>, field: impl IntoElement) -> Div {
    div().flex().items_center().gap(px(GAP)).child(div().w(px(WIDE_WIDTH)).child(label.into())).child(field)
}

fn cell_fields(editor: &Editor, area: &Area, cell: usize, face: Face, cx: &mut Context<Editor>) -> Div {
    let (column, row) = area.column_row(cell);
    let mut fields = div()
        .flex()
        .flex_col()
        .gap(px(GAP_SMALL))
        .child(strong(format!("Cell x {column} y {row}, {} face", face.label())));
    for corner in 0..CELL_BYTES {
        let text = area.bundle.cells[cell].heights[corner].to_string();
        let field = input(
            editor,
            ("area-corner", corner),
            Field::Area(Part::Corner(corner)),
            &text,
            "",
            Some(NUMBER_WIDTH),
            cx,
        );
        fields = fields.child(labelled(format!("Corner {corner}"), field));
    }
    let code = area.bundle.codes.get(cell).copied().unwrap_or(0);
    fields = fields.child(labelled(
        "Code",
        input(editor, "area-code", Field::Area(Part::Code), &format!("{code:#04X}"), "", Some(NUMBER_WIDTH), cx),
    ));
    if let Some(value) = area.battle_value(cell) {
        fields = fields.child(labelled(
            "Battle grid",
            input(editor, "area-battle", Field::Area(Part::Battle), &value.to_string(), "", Some(NUMBER_WIDTH), cx),
        ));
    }

    let Some(quad) = area.quads.iter().find(|quad| quad.cell == cell && quad.face == face) else {
        return fields.child(muted("The face is not drawn, so it has no descriptor."));
    };
    let Some(tile) = quad.tile else {
        return fields.child(muted("The face's descriptor is past the bundle's table."));
    };
    fields = fields.child(strong(format!("Descriptor {} = {:#010X}", tile.index, tile.descriptor))).child(muted(
        format!("{} drawn quads share it; an edit changes them all (func_80155560).", model::sharers(area, tile.index)),
    ));
    for field in TileField::ALL {
        let text = field.get(tile.descriptor).to_string();
        let id = ("area-tile", field as usize);
        fields = fields.child(labelled(
            field.label(),
            input(editor, id, Field::Area(Part::Tile(field)), &text, "", Some(NUMBER_WIDTH), cx),
        ));
    }
    fields
}

fn actor_fields(editor: &Editor, area: &Area, index: usize, cx: &mut Context<Editor>) -> Div {
    let actor = &area.actor_list()[index];
    let mut fields = div()
        .flex()
        .flex_col()
        .gap(px(GAP_SMALL))
        .child(strong(format!("Actor {index}: kind {:X} at program +{:#06X}", actor.record.kind, actor.record.at)));
    for line in model::describe(area, Hit::Actor(index)) {
        fields = fields.child(muted(line));
    }
    let placement = actor.placement;
    fields = fields
        .child(labelled(
            "Cell x",
            input(
                editor,
                "area-actor-x",
                Field::Area(Part::ActorX),
                &model::half(placement.x2),
                "",
                Some(NUMBER_WIDTH),
                cx,
            ),
        ))
        .child(labelled(
            "Cell y",
            input(
                editor,
                "area-actor-y",
                Field::Area(Part::ActorY),
                &model::half(placement.y2),
                "",
                Some(NUMBER_WIDTH),
                cx,
            ),
        ));
    if let Some(facing) = placement.facing {
        let hint = format!("0 to {}", FACINGS - 1);
        fields = fields.child(labelled(
            "Facing",
            input(editor, "area-facing", Field::Area(Part::Facing), &facing.to_string(), &hint, Some(NUMBER_WIDTH), cx),
        ));
    }
    if let Some(scene) = placement.scene {
        fields = fields.child(labelled(
            "Scene id",
            input(editor, "area-scene", Field::Area(Part::Scene), &format!("{scene:#06X}"), "", Some(WIDE_WIDTH), cx),
        ));
        let ids: Vec<String> = area.scenes.iter().map(|scene| format!("{:#06X}", scene.id)).collect();
        fields = fields.child(muted(format!("The scene table holds {}.", ids.join(", "))));
    }
    fields
}

fn encounter_fields(editor: &Editor, area: &Area, cx: &mut Context<Editor>) -> Div {
    let mut fields = div().flex().flex_col().gap(px(GAP_SMALL)).child(strong("Encounters (Area_PickEncounter)"));
    let Some(block) = area.encounter_block() else {
        return fields.child(muted("The archive has no encounter block at 0x800E4000."));
    };
    let names: Vec<String> =
        block.enemies.iter().enumerate().map(|(index, enemy)| format!("{index} {}", name_text(&enemy.name))).collect();
    fields = fields.child(muted(format!("Enemies: {}. 255 is an empty slot.", names.join(", "))));
    for (row, encounter) in block.encounters.iter().enumerate() {
        let mut line = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(GAP_SMALL))
            .child(div().w(px(GAP * 2.0)).child(row.to_string()));
        for slot in 0..=ENCOUNTER_ENEMIES {
            let value = if slot < ENCOUNTER_ENEMIES { encounter.enemies[slot] } else { encounter.appearance_rate };
            let id = ("area-encounter", row * (ENCOUNTER_ENEMIES + 1) + slot);
            line = line.child(input(
                editor,
                id,
                Field::Area(Part::Encounter { row, slot }),
                &value.to_string(),
                "",
                Some(px_width(slot)),
                cx,
            ));
        }
        fields = fields.child(line);
    }
    fields.child(muted("Eight enemy slots, then the appearance rate."))
}

/// The width of an encounter field: the rate is set apart.
fn px_width(slot: usize) -> f32 {
    if slot == ENCOUNTER_ENEMIES { NUMBER_WIDTH } else { NUMBER_WIDTH / 1.5 }
}

fn zone_list(area: &Area) -> Div {
    let mut list = div().flex().flex_col().gap(px(GAP_SMALL)).child(strong("Encounter zones (func_801B3C3C)"));
    match &area.zones {
        Ok(zones) => {
            for (index, zone) in zones.iter().enumerate() {
                list = list.child(mono(format!(
                    "{index}: ({}, {})-({}, {}) rate {} kind {} music {} {}",
                    zone.first[0],
                    zone.first[1],
                    zone.last[0],
                    zone.last[1],
                    zone.rate,
                    zone.kind,
                    zone.seed,
                    zone.pick
                )));
            }
            list.child(muted(
                "The zones live in SLUS_004.22, which the project does not rebuild, so they are read only.",
            ))
        }
        Err(error) => list.child(muted(format!("Unknown: {error}."))),
    }
}

/// A number typed as decimal or as `0x` hex.
fn number(text: &str) -> Option<u32> {
    let text = text.trim();
    match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => u32::from_str_radix(hex, 16).ok(),
        None => text.parse().ok(),
    }
}

/// Writes a value the user typed. Returns a status message when it cannot be written.
pub fn field_changed(editor: &mut Editor, part: Part, text: &str) -> Option<String> {
    let byte = || number(text).and_then(|value| u8::try_from(value).ok());
    match part {
        Part::CodeInk => {
            editor.views.area.code_ink = byte()?;
            return None;
        }
        Part::BattleInk => {
            editor.views.area.battle_ink = byte().filter(|&value| value <= 0x0F)?;
            return None;
        }
        _ => {}
    }
    let area = shown_area(editor)?;
    let result = match (part, editor.views.area.picked) {
        (Part::Corner(corner), Some(Target::Face { cell, .. })) => match text.trim().parse::<i8>() {
            Ok(height) => rewrite_bundle(editor, |payload| model::with_corner(payload, cell, corner, height)),
            Err(_) => Err("A corner height is a number from -128 to 127.".into()),
        },
        (Part::Code, Some(Target::Face { cell, .. })) => match byte() {
            Some(code) => rewrite_bundle(editor, |payload| model::with_code(payload, cell, code)),
            None => Err("A code is a byte, 0 to 0xFF.".into()),
        },
        (Part::Battle, Some(Target::Face { cell, .. })) => match byte() {
            Some(value) => {
                rewrite_sibling(editor, area.battle.as_ref().map(|(index, _)| *index), "battle grid", |grid| {
                    model::with_battle(grid, cell, value)
                })
            }
            None => Err("A battle grid value is a nibble, 0 to 15.".into()),
        },
        (Part::Tile(field), Some(Target::Face { cell, face })) => {
            let tile = area.quads.iter().find(|quad| quad.cell == cell && quad.face == face).and_then(|quad| quad.tile);
            match (tile, number(text)) {
                (Some(tile), Some(value)) => {
                    rewrite_bundle(editor, |payload| model::with_tile_field(payload, tile.index, field, value))
                }
                (None, _) => Err("The face has no descriptor.".into()),
                (_, None) => Err(format!("{} is a number.", field.label())),
            }
        }
        (Part::ActorX | Part::ActorY, Some(Target::Actor(index))) => match model::parse_half(text) {
            Some(value) => {
                let placement = area.actor_list().get(index).map(|actor| actor.placement);
                match placement {
                    Some(placement) => {
                        let (x2, y2) = if part == Part::ActorX { (value, placement.y2) } else { (placement.x2, value) };
                        rewrite_program(editor, &area, |area| model::with_actor_position(area, index, x2, y2))
                    }
                    None => Err("no such actor".into()),
                }
            }
            None => Err("A cell is a whole number, or one ending in .5 for half a cell on.".into()),
        },
        (Part::Facing, Some(Target::Actor(index))) => match byte() {
            Some(facing) => rewrite_program(editor, &area, |area| model::with_actor_facing(area, index, facing)),
            None => Err(format!("A facing is 0 to {}.", FACINGS - 1)),
        },
        (Part::Scene, Some(Target::Actor(index))) => match number(text).and_then(|value| u16::try_from(value).ok()) {
            Some(scene) => rewrite_program(editor, &area, |area| model::with_actor_scene(area, index, scene)),
            None => Err("A scene id is a 16-bit number.".into()),
        },
        (Part::Encounter { row, slot }, _) => match byte() {
            Some(value) => {
                rewrite_sibling(editor, area.encounters.as_ref().map(|(index, _)| *index), "encounter block", |block| {
                    model::with_encounter(block, row, slot, value)
                })
            }
            None => Err("An encounter slot is a byte, 0 to 255.".into()),
        },
        _ => Ok(()),
    };
    result.err()
}

#[cfg(test)]
mod tests;
