//! An encounter of an area as the field places it for battle (`rebof3_formats::formation`): each listed enemy at the
//! cell `Area_PlaceEncounterEnemies` gives it, drawn from its scene record through the area's replayed VRAM and
//! animated at the game's tick.

use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{AnyElement, Context, Div, canvas, div, fill, point, prelude::*, px, rgba, size};
use rebof3_emi::replay::scenes;
use rebof3_formats::clut::color_rgba;
use rebof3_formats::formation::{self, DIRECTIONS, Offsets, Placed};
use rebof3_formats::scene::SceneTable;
use rebof3_formats::script::program::Program;
use rebof3_formats::sprite::{
    Animation, Bounds, Playback, Player, SpriteSizes, Stage, TICK_MICROS, animation_bounds, compose,
};
use rebof3_formats::{Format, classify};

use super::Entry;
use super::texture::paint_pixels;
use crate::app::Editor;
use crate::project::ArchivePath;
use crate::theme::{BACKGROUND, color};
use crate::widgets::{GAP, GAP_SMALL, button, checkbox, mono, muted, separator, tab};

/// The archive whose entry 0 holds `s_EncounterEnemyOffsets`, and the executable with `g_SpriteSizes`.
const GAME: &str = "BIN/ETC/GAME.EMI";
const EXECUTABLE: &str = "SLUS_004.22";
/// The load addresses of the area's scene table and script table.
const SCENE_TABLE: u32 = 0x800E_3800;
const SCRIPT_TABLE: u32 = 0x800D_3800;
/// Screen pixels per map cell in the top-down layout, and the 16.16 fraction bits of a cell.
const CELL_PIXELS: i32 = 32;
const CELL_SHIFT: u32 = 16;
const ZOOM: f32 = 2.0;
/// The most ticks one timer wake plays.
const CATCH_UP_TICKS: u64 = 64;
const ORIGIN_LINE: u32 = 0xff5050ff;
const CROSS: f32 = 6.0;
const LINE: f32 = 1.0;

/// One enemy of the shown formation, ready to draw.
struct Shown {
    placed: Placed,
    /// The stage and the stream, when the scene record and the script table give them.
    sprite: Option<(Stage, Animation)>,
    player: Option<Player>,
}

/// What the view read for one entry, encounter, direction and record.
struct Loaded {
    key: (String, usize, u64, usize, u8, usize),
    enemies: Result<Vec<Shown>, String>,
    scene: String,
}

#[derive(Default)]
pub struct State {
    encounter: usize,
    direction: u8,
    /// The script table entry every enemy plays.
    record: usize,
    mirror: bool,
    clock: Option<(Instant, u64, u64)>,
    generation: u64,
    loaded: Option<Rc<Loaded>>,
    sizes: Option<Result<SpriteSizes, String>>,
}

/// Places the shown encounter and stages each enemy, when what they read changed.
pub fn prepare(editor: &mut Editor) {
    let Some(entry) = editor.current() else { return };
    let Some(project) = editor.project.as_ref() else { return };
    let state = &editor.views.formation;
    let archive = entry.archive.path.0.clone();
    let key = (archive.clone(), entry.index, project.revision(), state.encounter, state.direction, state.record);
    if state.loaded.as_ref().is_some_and(|loaded| loaded.key == key) {
        return;
    }
    let block = entry.payload().to_vec();
    let sibling = |load: u32| {
        (0..entry.archive.entries().len())
            .find(|&index| {
                let payload = entry.sibling(index);
                project
                    .entries_of(entry.archive)
                    .get(index)
                    .is_some_and(|table| table.ram_ptr == load && classify(table, payload) != Format::Reserve)
            })
            .map(|index| entry.sibling(index).to_vec())
    };
    let table = sibling(SCENE_TABLE).and_then(|bytes| SceneTable::decode(&bytes).ok());
    let program = sibling(SCRIPT_TABLE).and_then(|bytes| Program::decode(&bytes).ok());
    let disc = project.disc.clone();

    let Some(project) = editor.project.as_mut() else { return };
    let offsets = project.load(&ArchivePath(GAME.into())).and_then(|game| {
        let entry = project.entries_of(&game).first().cloned().ok_or_else(|| "GAME.EMI has no entries".to_string())?;
        Offsets::from_code(project.payload(&game, 0), entry.ram_ptr).map_err(|error| error.to_string())
    });
    let scene = scenes::field(&archive);
    let replayed = super::scene::replay(project, &scene);

    let enemies = offsets.and_then(|offsets| {
        let (encounter, direction, record) = (key.3, key.4, key.5);
        let placed = formation::place(&block, table.as_ref(), encounter, &offsets, direction)
            .map_err(|error| error.to_string())?;
        Ok(placed
            .into_iter()
            .map(|placed| {
                let sprite = placed.scene.as_ref().and_then(|record_of| {
                    let animation = formation::stream(program.as_ref()?, record_of, record)?.clone();
                    Some((Stage::resident(replayed.vram.clone(), formation::placement(record_of)), animation))
                });
                let player = sprite.as_ref().map(|(_, animation)| Player::start(animation));
                Shown { placed, sprite, player }
            })
            .collect())
    });

    let state = &mut editor.views.formation;
    if state.sizes.is_none() {
        state.sizes = Some(
            std::fs::read(disc.join(EXECUTABLE))
                .map_err(|error| format!("{EXECUTABLE}: {error}"))
                .and_then(|exe| SpriteSizes::from_executable(&exe).map_err(|error| error.to_string())),
        );
    }
    state.loaded = Some(Rc::new(Loaded { key, enemies, scene: scene.name }));
}

pub fn show(editor: &Editor, _entry: &Entry<'_>, cx: &mut Context<Editor>) -> AnyElement {
    let state = &editor.views.formation;
    let Some(loaded) = state.loaded.as_ref() else { return muted("Placing the formation…").into_any_element() };

    let mut encounters = div().flex().flex_wrap().items_center().gap(px(GAP)).child("Encounter");
    for number in 0..rebof3_formats::encounter::ENCOUNTERS {
        encounters =
            encounters.child(tab(("encounter", number), number.to_string(), number == state.encounter).on_click(
                cx.listener(move |editor, _, _, cx| {
                    editor.views.formation.encounter = number;
                    editor.views.formation.clock = None;
                    cx.notify();
                }),
            ));
    }
    encounters = encounters.child(separator(true)).child("Scroll direction");
    for direction in 0..DIRECTIONS {
        encounters = encounters.child(
            tab(("direction", usize::from(direction)), direction.to_string(), direction == state.direction).on_click(
                cx.listener(move |editor, _, _, cx| {
                    editor.views.formation.direction = direction;
                    cx.notify();
                }),
            ),
        );
    }

    let playing = state.clock.is_some();
    let transport = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(GAP))
        .child(button("formation-play", if playing { "Pause" } else { "Play" }, true).on_click(cx.listener(
            |editor, _, _, cx| {
                if editor.views.formation.clock.is_some() {
                    editor.views.formation.clock = None;
                } else {
                    play(editor, cx);
                }
                cx.notify();
            },
        )))
        .child(separator(true))
        .child("Script entry")
        .child(button("record-down", "−", state.record > 0).on_click(cx.listener(|editor, _, _, cx| {
            let state = &mut editor.views.formation;
            state.record = state.record.saturating_sub(1);
            cx.notify();
        })))
        .child(mono(state.record.to_string()))
        .child(button("record-up", "+", true).on_click(cx.listener(|editor, _, _, cx| {
            editor.views.formation.record += 1;
            cx.notify();
        })))
        .child(checkbox("formation-mirror", "Mirror", state.mirror).on_click(cx.listener(|editor, _, _, cx| {
            editor.views.formation.mirror = !editor.views.formation.mirror;
            cx.notify();
        })));

    let body = match &loaded.enemies {
        Err(error) => muted(format!("The formation does not place: {error}")),
        Ok(enemies) if enemies.is_empty() => muted("This encounter lists no enemies."),
        Ok(enemies) => match &state.sizes {
            Some(Ok(sizes)) => formation_view(enemies, sizes, state.mirror, &loaded.scene),
            Some(Err(error)) => muted(format!("Frames are not drawn: g_SpriteSizes does not read ({error}).")),
            None => muted("Reading g_SpriteSizes…"),
        },
    };
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .gap(px(GAP))
        .child(encounters)
        .child(transport)
        .child(muted(
            "Placed by Area_PlaceEncounterEnemies from s_EncounterEnemyOffsets, every listed slot kept; drawn by \
             Battle_InitEnemy's scene record through the area's VRAM. Top-down on the cell grid, the party's cell \
             marked in red. The record the battle plays is not traced: pick a script entry.",
        ))
        .child(separator(false))
        .child(div().id("formation-body").flex_1().min_h_0().overflow_y_scroll().child(body))
        .into_any_element()
}

/// The enemies drawn at their cells, and one line per enemy.
fn formation_view(enemies: &[Shown], sizes: &SpriteSizes, mirror: bool, scene: &str) -> Div {
    // Each enemy's animation box, moved to its cell.
    let place = |shown: &Shown| {
        let x = (shown.placed.x * CELL_PIXELS) >> CELL_SHIFT;
        let y = (shown.placed.y * CELL_PIXELS) >> CELL_SHIFT;
        (x, y)
    };
    let mut total = Bounds { left: -CELL_PIXELS, top: -CELL_PIXELS, right: CELL_PIXELS, bottom: CELL_PIXELS };
    let mut boxes = Vec::new();
    for shown in enemies {
        let (x, y) = place(shown);
        let bounds = shown
            .sprite
            .as_ref()
            .and_then(|(_, animation)| animation_bounds(animation, sizes, mirror))
            .unwrap_or(Bounds { left: -1, top: -1, right: 1, bottom: 1 });
        let moved =
            Bounds { left: bounds.left + x, top: bounds.top + y, right: bounds.right + x, bottom: bounds.bottom + y };
        total = total.union(moved);
        boxes.push((bounds, moved));
    }

    let (width, height) = (total.width(), total.height());
    let mut pixels = vec![[0u8; 4]; width * height];
    let mut lines = Vec::new();
    for (shown, (bounds, moved)) in enemies.iter().zip(&boxes) {
        let placed = &shown.placed;
        let name = format!(
            "slot {} enemy {} {:<8} scene id {:#06X} at ({:+.1}, {:+.1}) cells",
            placed.slot,
            placed.enemy,
            placed.name,
            placed.scene_id,
            f64::from(placed.x) / f64::from(1 << CELL_SHIFT),
            f64::from(placed.y) / f64::from(1 << CELL_SHIFT)
        );
        let (Some((stage, animation)), Some(player)) = (&shown.sprite, shown.player) else {
            let why = if placed.scene.is_none() {
                "the area has no scene record with this id"
            } else {
                "the script entry is not a stream"
            };
            lines.push(format!("{name}: not drawn, {why}"));
            continue;
        };
        let Some(frame) = animation.frames.get(player.frame()) else { continue };
        let composite = compose(stage, frame, sizes, mirror, *bounds);
        for (index, pixel) in composite.pixels.iter().enumerate() {
            let Some(pixel) = pixel.filter(|pixel| pixel.texel.drawn()) else { continue };
            let (x, y) = (index % bounds.width(), index / bounds.width());
            let (x, y) = (moved.left - total.left + x as i32, moved.top - total.top + y as i32);
            if let Some(slot) = pixels.get_mut(y as usize * width + x as usize) {
                *slot = color_rgba(pixel.texel.word);
            }
        }
        let record = placed.scene.as_ref().map(|record| {
            format!(
                "page {} row {} CLUT selector {:#04X} kind {} section {}",
                record.texture_page, record.texture_v, record.clut_select, record.texture_mode, record.script_section
            )
        });
        lines.push(format!(
            "{name}: frame {} of {}, {}",
            player.frame(),
            animation.frames.len(),
            record.unwrap_or_default()
        ));
    }

    let origin = (-total.left, -total.top);
    let picture = canvas(
        |_, _, _| {},
        move |area, (), window, _| {
            paint_pixels(window, area, &pixels, width, ZOOM);
            let centre = point(area.origin.x + px(origin.0 as f32 * ZOOM), area.origin.y + px(origin.1 as f32 * ZOOM));
            let (arm, thin) = (px(CROSS), px(LINE));
            window.paint_quad(fill(
                gpui::Bounds::new(point(centre.x - arm, centre.y), size(arm * 2.0, thin)),
                rgba(ORIGIN_LINE),
            ));
            window.paint_quad(fill(
                gpui::Bounds::new(point(centre.x, centre.y - arm), size(thin, arm * 2.0)),
                rgba(ORIGIN_LINE),
            ));
        },
    )
    .w(px(width as f32 * ZOOM))
    .h(px(height as f32 * ZOOM))
    .flex_none();

    let mut list = div().flex().flex_col().gap(px(GAP_SMALL)).child(muted(format!("VRAM of scene {scene}.")));
    for line in lines {
        list = list.child(mono(line));
    }
    div()
        .flex()
        .flex_col()
        .gap(px(GAP))
        .child(div().flex_none().p(px(GAP_SMALL)).bg(color(BACKGROUND)).child(picture))
        .child(list)
}

/// Starts the shared clock: every enemy steps its stream once a tick.
fn play(editor: &mut Editor, cx: &mut Context<Editor>) {
    let state = &mut editor.views.formation;
    state.generation += 1;
    let generation = state.generation;
    state.clock = Some((Instant::now(), 0, generation));
    let wake = Duration::from_micros(TICK_MICROS as u64);
    cx.spawn(async move |this, cx| {
        loop {
            cx.background_executor().timer(wake).await;
            let Ok(playing) = this.update(cx, |editor, cx| {
                let playing = advance(editor, generation);
                cx.notify();
                playing
            }) else {
                break;
            };
            if !playing {
                break;
            }
        }
    })
    .detach();
}

/// Plays the ticks due since play `generation` started. Returns false once that play stopped.
fn advance(editor: &mut Editor, generation: u64) -> bool {
    let state = &mut editor.views.formation;
    let Some((started, played, current)) = state.clock.filter(|clock| clock.2 == generation) else { return false };
    let due = (started.elapsed().as_secs_f64() * 1_000_000.0 / TICK_MICROS) as u64;
    let Some(loaded) = state.loaded.as_mut().and_then(Rc::get_mut) else { return false };
    if let Ok(enemies) = &mut loaded.enemies {
        for _ in 0..due.saturating_sub(played).min(CATCH_UP_TICKS) {
            for shown in enemies.iter_mut() {
                if let (Some((_, animation)), Some(player)) = (&shown.sprite, shown.player.as_mut()) {
                    player.tick(animation, Playback::Loop);
                }
            }
        }
    }
    state.clock = Some((started, due, current));
    true
}

#[cfg(test)]
impl State {
    /// The enemies the view placed, with whether each is drawn.
    pub fn summary(&self) -> Option<Vec<(String, bool)>> {
        let enemies = self.loaded.as_ref()?.enemies.as_ref().ok()?;
        Some(enemies.iter().map(|shown| (shown.placed.name.clone(), shown.sprite.is_some())).collect())
    }
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;
    use rebof3_formats::Format;

    use super::super::Tab;
    use crate::app::Editor;
    use crate::project::ArchivePath;

    /// AREA003's encounter 2 places its goblins and draws each through the area's VRAM.
    #[gpui::test]
    fn a_retail_formation_places_and_draws(cx: &mut TestAppContext) {
        let Some(disc) = crate::retail_support::retail_disc() else {
            return;
        };
        let folder = std::env::temp_dir().join(format!("bof3-edit-formation-{}", std::process::id()));
        let project = folder.clone();
        let (editor, cx) = cx.add_window_view(|_, cx| Editor::new(Some(disc), Some(project), cx));
        editor.update(cx, |editor, _| {
            editor.select_archive(&ArchivePath("BIN/WORLD00/AREA003.EMI".into()));
            let index = editor
                .archive
                .as_ref()
                .expect("the area loads")
                .formats
                .iter()
                .position(|format| *format == Format::AreaEncounterTable);
            editor.select_entry(index.expect("the area has an encounter block"));
            editor.tab = Tab::Formation;
            editor.views.formation.encounter = 2;
        });
        cx.update(|window, cx| window.draw(cx).clear());
        let summary = editor.update(cx, |editor, _| editor.views.formation.summary()).expect("the formation places");
        assert!(!summary.is_empty() && summary.iter().all(|(_, drawn)| *drawn), "{summary:?}");
        assert!(summary.iter().any(|(name, _)| name == "Goblin"), "{summary:?}");
        let _ = std::fs::remove_dir_all(&folder);
    }
}
