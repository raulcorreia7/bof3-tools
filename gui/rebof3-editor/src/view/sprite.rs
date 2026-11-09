//! A sprite bank as the game shows it (`rebof3_emi::sprite`, `wiki: formats/character`): each section's
//! animations played at the game's tick, every frame stitched through the VRAM model, the frame sheet
//! the cells cut from, and the palettes.
//!
//! The frame is drawn from a [`Stage`]: the sheet and the palette written into VRAM where the bank's
//! reader puts them, then read back through the texture page and CLUT the draw sets. The palette is
//! the one the reader loads when the reader is traced ([`Reader::palette`]); otherwise the frame is
//! drawn in grey by texel index and the palettes are listed as a ranked suggestion that the user
//! applies. A pixel whose colour the staged palette does not give is drawn in the marker colour, and
//! cell texels outside the sheet are counted, not guessed.
//!
//! A palette edit writes the colour in place; an imported sprite sheet rewrites the section. Both go
//! back through `rebof3_assets::sprites`, so an unchanged bank keeps its stored bytes and an LZ77
//! entry is packed again only when it changed.

use std::cell::Cell as Shared;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, Bounds as Area, Context, Div, Pixels, Window, canvas, div, fill, point, prelude::*, px, rgba, size,
};
use rebof3_assets::decode::color_rgba;
use rebof3_assets::sprites::{self, SpriteSizes};
use rebof3_emi::replay::Vram;
use rebof3_emi::sprite::{
    self as format, Animation, Bank, Bounds, COLOURS, Frame, Image, Palette, Placement, Playback, Player, Reader,
    Section, Stage, TICK_MICROS, animation_bounds, cell_rect, compose, duration_ms, frame_bounds, ticks_of,
};

use super::Entry;
use super::palette::{SWATCH, SWATCH_GAP, Sliders, picker};
use super::texture::paint_pixels;
use crate::app::Editor;
use crate::theme::{BACKGROUND, MUTED, color};
use crate::widgets::{
    GAP, GAP_SMALL, button, checkbox, mono, muted, outline, selectable, separator, slider, strong, swatch, tab, tip,
};

/// The executable in the disc folder that holds `g_SpriteSizes`.
const EXECUTABLE: &str = "SLUS_004.22";
const ZOOM_MIN: u32 = 1;
const ZOOM_MAX: u32 = 8;
const ZOOM_DEFAULT: u32 = 3;
/// The most ticks one timer wake plays, so a window that stalled does not run the animation on for long.
const CATCH_UP_TICKS: u64 = 64;
/// The most ticks a step or a seek plays looking for its step: every step's longest delay, twice over.
const SEEK_TICKS_PER_STEP: usize = 2 * 256;
/// The colour of an index the staged palette does not give: opaque magenta, as the sprite sheet marks it.
const MARKER: [u8; 4] = [0xFF, 0x00, 0xFF, 0xFF];
/// The colour of a pixel that draws nothing.
const CLEAR: [u8; 4] = [0, 0, 0, 0];
/// The brightest grey level, and the alpha of a grey pixel.
const GREY_MAX: usize = 255;
const OPAQUE: u8 = 255;
/// The outline colours: every cell, the frame's box, and the actor's position.
const CELL_LINE: u32 = 0x4ec9b0ff;
const FRAME_LINE: u32 = 0xffe650ff;
const ORIGIN_LINE: u32 = 0xff5050ff;
/// The width of an outline, and the half-length of the origin cross, in screen pixels.
const LINE: f32 = 1.0;
const CROSS: f32 = 6.0;
/// The width of the animation list.
const LIST_WIDTH: f32 = 330.0;

/// A palette the frame is drawn with.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Colours {
    /// Palette `n` of the section.
    Palette(usize),
    /// Grey by texel index, for a reader whose palette is not traced.
    Grey,
}

/// The bank of the shown entry, decoded once per entry and project revision.
struct Loaded {
    key: (String, usize, u64),
    bank: Result<Bank, String>,
    reader: Reader,
}

/// The sheet and palette staged into VRAM, once per section, sheet and palette.
struct Staged {
    key: (usize, usize, usize),
    stage: Stage,
}

/// When playback started: the instant and the ticks played since, and the play it belongs to.
#[derive(Clone, Copy)]
struct Clock {
    started: Instant,
    ticks: u64,
    generation: u64,
}

pub struct State {
    section: usize,
    animation: usize,
    /// The palette the user picked; `None` draws with the reader's palette, or grey without one.
    palette: Option<usize>,
    mirror: bool,
    outlines: bool,
    zoom: u32,
    zoom_slider: Rc<Shared<Area<Pixels>>>,
    playback: Playback,
    /// The stream state of the shown animation, and the section and animation it plays.
    player: Option<(usize, usize, Player)>,
    /// Set while the animation plays.
    clock: Option<Clock>,
    /// Counts plays, so a timer of an earlier play stops.
    generation: u64,
    /// The colour the picker edits: a palette of the section and a colour of it.
    colour: (usize, usize),
    sliders: Sliders,
    loaded: Option<Rc<Loaded>>,
    staged: Option<Rc<Staged>>,
    /// `g_SpriteSizes`, read once per disc folder.
    sizes: Option<(PathBuf, Result<SpriteSizes, String>)>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            section: 0,
            animation: 0,
            palette: None,
            mirror: false,
            outlines: true,
            zoom: ZOOM_DEFAULT,
            zoom_slider: Rc::default(),
            playback: Playback::default(),
            player: None,
            clock: None,
            generation: 0,
            colour: (0, 0),
            sliders: Sliders::default(),
            loaded: None,
            staged: None,
            sizes: None,
        }
    }
}

impl State {
    /// The bank, when it decodes.
    fn bank(&self) -> Option<&Bank> {
        self.loaded.as_ref()?.bank.as_ref().ok()
    }

    /// The shown section.
    fn section(&self) -> Option<&Section> {
        self.bank()?.sections.get(self.section)
    }

    /// The shown animation.
    fn animation(&self) -> Option<&Animation> {
        self.section()?.animations.get(self.animation)
    }

    fn sizes(&self) -> Option<&SpriteSizes> {
        self.sizes.as_ref()?.1.as_ref().ok()
    }

    /// The palette the frame is drawn with: the user's pick, else the reader's, else grey.
    fn colours(&self) -> Colours {
        let reader = self.loaded.as_ref().map_or(Reader::Unknown, |loaded| loaded.reader);
        match self.palette.or(reader.palette()) {
            Some(palette) => Colours::Palette(palette),
            None => Colours::Grey,
        }
    }

    fn player(&self) -> Option<Player> {
        self.player
            .filter(|(section, animation, _)| (*section, *animation) == (self.section, self.animation))
            .map(|(_, _, player)| player)
    }

    /// One tick of the shown animation. Returns false when there is none.
    fn tick(&mut self) -> bool {
        let Some(mut player) = self.player() else { return false };
        let playback = self.playback;
        let Some(animation) = self.animation() else { return false };
        player.tick(animation, playback);
        self.player = Some((self.section, self.animation, player));
        true
    }

    /// The shown animation from its first step.
    fn restart(&mut self) {
        self.player = self.animation().map(|animation| (self.section, self.animation, Player::start(animation)));
    }

    /// Plays ticks until the step on screen is `target`, or until a bound when the stream never shows it.
    fn seek(&mut self, target: usize) {
        self.restart();
        let bound = self.animation().map_or(0, |animation| animation.steps.len() * SEEK_TICKS_PER_STEP);
        for _ in 0..bound {
            if self.player().is_none_or(|player| player.step() == target) {
                return;
            }
            self.tick();
        }
    }

    /// Plays ticks until the step on screen changes.
    fn step(&mut self) {
        let Some(start) = self.player() else { return };
        for _ in 0..SEEK_TICKS_PER_STEP {
            if !self.tick() {
                return;
            }
            let player = self.player().unwrap_or(start);
            if player.step() != start.step() || player.ended() {
                return;
            }
        }
    }

    /// What the view shows, in one line, once it is drawn.
    #[cfg(test)]
    pub fn summary(&self) -> Option<String> {
        let section = self.section()?;
        let player = self.player()?;
        Some(format!(
            "section {} of {}, animation {} of {}, frame {}, tick {}",
            self.section,
            self.bank()?.sections.len(),
            self.animation,
            section.animations.len(),
            player.frame(),
            player.ticks()
        ))
    }
}

/// Decodes the shown bank, reads `g_SpriteSizes` and stages the shown sheet and palette, each when what
/// it reads changed.
pub fn prepare(editor: &mut Editor) {
    let Some(entry) = editor.current() else { return };
    let Some(project) = editor.project.as_ref() else { return };
    let key = (entry.archive.path.0.clone(), entry.index, project.revision());
    let disc = project.disc.clone();
    let fresh = editor.views.sprite.loaded.as_ref().is_none_or(|loaded| loaded.key != key).then(|| {
        let bank = sprites::unpack(entry.payload())
            .and_then(|(_, bytes)| Ok(format::decode(&bytes)?))
            .map_err(|error| error.to_string());
        let reader = Reader::of(&key.0);
        Rc::new(Loaded { key, bank, reader })
    });
    let state = &mut editor.views.sprite;

    if let Some(fresh) = fresh {
        let same_entry =
            state.loaded.as_ref().is_some_and(|loaded| (&loaded.key.0, loaded.key.1) == (&fresh.key.0, fresh.key.1));
        if !same_entry {
            *state = State { sizes: state.sizes.take(), ..State::default() };
        }
        state.loaded = Some(fresh);
        state.staged = None;
    }
    if state.sizes.as_ref().is_none_or(|(folder, _)| *folder != disc) {
        let sizes = std::fs::read(disc.join(EXECUTABLE))
            .map_err(|error| format!("{EXECUTABLE}: {error}"))
            .and_then(|exe| sprites::sprite_sizes(&exe).map_err(|error| error.to_string()));
        state.sizes = Some((disc, sizes));
    }

    let Some(loaded) = state.loaded.clone() else { return };
    let Some(section) = loaded.bank.as_ref().ok().and_then(|bank| bank.sections.get(state.section)) else { return };
    state.animation = state.animation.min(section.animations.len().saturating_sub(1));
    if state.palette.is_some_and(|palette| palette >= section.palettes.len()) {
        state.palette = None;
    }
    if state.player().is_none() {
        state.restart();
    }
    let palette = match state.colours() {
        Colours::Palette(palette) => palette,
        // Grey still stages a palette, so the stage knows which CLUT words are the bank's.
        Colours::Grey => 0,
    };
    let (Some(sheet), Some(colours)) = (section.image_of(state.animation), section.palettes.get(palette)) else {
        state.staged = None;
        return;
    };
    let staged_key = (state.section, sheet, palette);
    if state.staged.as_ref().is_none_or(|staged| staged.key != staged_key) {
        let stage = Stage::new(Vram::new(), (sheet, &section.images[sheet]), (palette, colours), Placement::default());
        state.staged = Some(Rc::new(Staged { key: staged_key, stage }));
    }
}

pub fn show(editor: &Editor, entry: &Entry<'_>, cx: &mut Context<Editor>) -> AnyElement {
    let state = &editor.views.sprite;
    let Some(loaded) = state.loaded.as_ref() else { return muted("Reading the bank…").into_any_element() };
    let bank = match &loaded.bank {
        Ok(bank) => bank,
        Err(error) => return muted(format!("Not a sprite bank: {error}")).into_any_element(),
    };
    let Some(section) = bank.sections.get(state.section) else {
        return muted("The bank has no sections.").into_any_element();
    };

    let mut sections = div().flex().flex_wrap().items_center().gap(px(GAP)).child("Section");
    for number in 0..bank.sections.len() {
        sections = sections.child(tab(("section", number), number.to_string(), number == state.section).on_click(
            cx.listener(move |editor, _, _, cx| {
                let state = &mut editor.views.sprite;
                (state.section, state.animation, state.colour, state.clock) = (number, 0, (0, 0), None);
                cx.notify();
            }),
        ));
    }
    sections = sections.child(separator(true)).child(muted(reader_note(loaded.reader)));

    let body = div()
        .flex()
        .flex_1()
        .min_h_0()
        .gap(px(GAP))
        .child(animation_list(state, section, cx))
        .child(separator(true))
        .child(player_panel(editor, entry, section, cx));
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .gap(px(GAP))
        .child(sections)
        .child(palette_line(state, section, loaded.reader, cx))
        .child(separator(false))
        .child(body)
        .into_any_element()
}

/// What the reader of the bank proves.
fn reader_note(reader: Reader) -> String {
    match reader {
        Reader::Field => "Party bank: func_801C1254 places it and loads palette 0.".into(),
        Reader::Battle => "Battle bank: its reader is not traced; drawn with the field placement.".into(),
        Reader::Unknown => "No traced loader reads this archive; drawn with the field placement.".into(),
    }
}

/// The palette chooser, with the ranked suggestion when the reader's palette is not traced.
fn palette_line(state: &State, section: &Section, reader: Reader, cx: &mut Context<Editor>) -> Div {
    let shown = state.colours();
    let mut line = div().flex().flex_wrap().items_center().gap(px(GAP)).child("Palette");
    for palette in 0..section.palettes.len() {
        let label = if reader.palette() == Some(palette) { format!("{palette} (loaded)") } else { palette.to_string() };
        line = line.child(tab(("palette", palette), label, shown == Colours::Palette(palette)).on_click(cx.listener(
            move |editor, _, _, cx| {
                editor.views.sprite.palette = Some(palette);
                cx.notify();
            },
        )));
    }
    if reader.palette().is_none() {
        line = line.child(tab("grey", "grey by index", shown == Colours::Grey).on_click(cx.listener(
            |editor, _, _, cx| {
                editor.views.sprite.palette = None;
                cx.notify();
            },
        )));
        if shown == Colours::Grey {
            line = line.child(muted(suggestion(section)));
        }
    }
    line
}

/// The ranked palette suggestion for a reader that is not traced.
fn suggestion(section: &Section) -> String {
    let rest = (1..section.palettes.len()).map(|palette| palette.to_string()).collect::<Vec<_>>().join(", ");
    let rest = if rest.is_empty() { String::new() } else { format!(", then {rest} in table order") };
    format!("Suggested, not applied: palette {} (the one func_801C1254 loads){rest}.", format::FIELD_PALETTE)
}

/// Every animation of the section, with its sheet, frames, steps, loop point and the records that play it.
fn animation_list(state: &State, section: &Section, cx: &mut Context<Editor>) -> impl IntoElement {
    let mut list = div().id("sprite-animations").flex().flex_col().w(px(LIST_WIDTH)).flex_none().overflow_y_scroll();
    for (index, animation) in section.animations.iter().enumerate() {
        let records: Vec<String> = section
            .records
            .iter()
            .enumerate()
            .filter(|(_, record)| usize::from(record.animation) == index)
            .map(|(record, _)| record.to_string())
            .collect();
        let sheet = section.image_of(index).map_or("no record plays it".to_string(), |sheet| format!("sheet {sheet}"));
        let looping = animation.loop_to().map_or(String::new(), |to| format!(", loops to {to}"));
        let records = if records.is_empty() { String::new() } else { format!(", records {}", records.join(" ")) };
        let line = format!(
            "{index:>3}  {sheet}, {} frames, {} steps{looping}{records}",
            animation.frames.len(),
            animation.steps.len()
        );
        list = list.child(selectable(("animation", index), index == state.animation).child(mono(line)).on_click(
            cx.listener(move |editor, _, _, cx| {
                editor.views.sprite.animation = index;
                editor.views.sprite.clock = None;
                cx.notify();
            }),
        ));
    }
    list
}

/// The transport, the readout, the frame and sheet, the steps and the palette editor.
fn player_panel(editor: &Editor, entry: &Entry<'_>, section: &Section, cx: &mut Context<Editor>) -> impl IntoElement {
    let state = &editor.views.sprite;
    let mut panel = div()
        .id("sprite-player")
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .gap(px(GAP))
        .overflow_y_scroll()
        .child(transport(state, entry, cx));

    let sizes = match &state.sizes {
        Some((_, Ok(sizes))) => Some(*sizes),
        Some((_, Err(error))) => {
            panel = panel.child(muted(format!("Frames are not drawn: g_SpriteSizes does not read ({error}).")));
            None
        }
        None => None,
    };
    let animation = section.animations.get(state.animation);
    let player = state.player();
    if let (Some(animation), Some(player)) = (animation, player) {
        panel = panel.child(mono(readout(animation, player)).text_color(color(MUTED)));
    }
    if let (Some(sizes), Some(animation), Some(player)) = (sizes, animation, player) {
        panel = panel.child(stage_view(state, section, animation, player, &sizes));
    }
    if let Some(animation) = animation {
        panel = panel.child(step_list(animation, player, cx));
    }
    panel.child(separator(false)).child(palette_editor(state, section, entry, cx))
}

/// Play and pause, tick, step, restart, the playback mode, mirror, outlines, zoom, export and import.
fn transport(state: &State, entry: &Entry<'_>, cx: &mut Context<Editor>) -> Div {
    let playing = state.clock.is_some();
    let mut row = div().flex().flex_wrap().items_center().gap(px(GAP));
    row = row
        .child(button("play", if playing { "Pause" } else { "Play" }, true).on_click(cx.listener(
            move |editor, _, _, cx| {
                if editor.views.sprite.clock.is_some() {
                    editor.views.sprite.clock = None;
                } else {
                    play(editor, cx);
                }
                cx.notify();
            },
        )))
        .child(button("tick", "Tick", !playing).tooltip(tip("One game tick (two video fields).")).on_click(
            cx.listener(|editor, _, _, cx| {
                editor.views.sprite.tick();
                cx.notify();
            }),
        ))
        .child(button("step", "Step", !playing).tooltip(tip("Play ticks until the next step shows.")).on_click(
            cx.listener(|editor, _, _, cx| {
                editor.views.sprite.step();
                cx.notify();
            }),
        ))
        .child(button("restart", "Restart", true).on_click(cx.listener(|editor, _, _, cx| {
            editor.views.sprite.restart();
            if editor.views.sprite.clock.is_some() {
                play(editor, cx);
            }
            cx.notify();
        })))
        .child(separator(true));
    for playback in Playback::ALL {
        row = row.child(
            tab(("playback", playback as usize), playback.label(), state.playback == playback)
                .tooltip(tip(playback_rule(playback)))
                .on_click(cx.listener(move |editor, _, _, cx| {
                    editor.views.sprite.playback = playback;
                    cx.notify();
                })),
        );
    }
    let zoom = slider("sprite-zoom", state.zoom, ZOOM_MIN..=ZOOM_MAX, &state.zoom_slider, cx, |editor, zoom| {
        editor.views.sprite.zoom = zoom;
    });
    let exe = entry.project.disc.join(EXECUTABLE);
    let can_export = state.sizes().is_some() && matches!(state.colours(), Colours::Palette(_));
    row.child(separator(true))
        .child(checkbox("mirror", "Mirror", state.mirror).on_click(cx.listener(|editor, _, _, cx| {
            editor.views.sprite.mirror = !editor.views.sprite.mirror;
            cx.notify();
        })))
        .child(checkbox("outlines", "Cell outlines", state.outlines).on_click(cx.listener(|editor, _, _, cx| {
            editor.views.sprite.outlines = !editor.views.sprite.outlines;
            cx.notify();
        })))
        .child("Zoom")
        .child(zoom)
        .child(mono(format!("{}×", state.zoom)))
        .child(separator(true))
        .child(
            button("export-sheet", "Export sheet…", can_export)
                .tooltip(tip("The section as a PNG sprite sheet and Aseprite JSON, drawn with the shown palette."))
                .on_click(cx.listener(move |editor, _, _, cx| export_sheet(editor, cx))),
        )
        .child(
            button("import-sheet", "Import sheet…", state.sizes().is_some())
                .tooltip(tip(format!("Read an edited sheet back into its section; needs {}.", exe.display())))
                .on_click(cx.listener(|editor, _, _, cx| import_sheet(editor, cx))),
        )
}

/// The step function a playback mode stands for.
fn playback_rule(playback: Playback) -> &'static str {
    match playback {
        Playback::Loop => "func_8014D978: wraps to the first step and follows jumps.",
        Playback::Once => "func_8014DAEC: holds the last step and follows jumps.",
        Playback::Reverse => "func_8014DA24: plays the steps backwards and ignores jumps.",
    }
}

/// The position in the stream, in ticks and milliseconds.
fn readout(animation: &Animation, player: Player) -> String {
    let ms = |ticks: f64| (ticks * TICK_MICROS / 1000.0).round();
    let left = if player.delay() == 0 { 256 } else { u32::from(player.delay()) };
    format!(
        "tick {} ({} ms)   step {} of {}   frame {} of {}   {left} ticks left ({} ms){}",
        player.ticks(),
        ms(player.ticks() as f64),
        player.step(),
        animation.steps.len(),
        player.frame(),
        animation.frames.len(),
        ms(f64::from(left)),
        if player.ended() { "   end of stream" } else { "" }
    )
}

/// The frame on screen over the box of every frame, and the sheet with the frame's cells outlined.
fn stage_view(state: &State, section: &Section, animation: &Animation, player: Player, sizes: &SpriteSizes) -> Div {
    let row = div().flex().flex_wrap().items_start().gap(px(GAP));
    let Some(frame) = animation.frames.get(player.frame()) else {
        return row.child(muted(format!(
            "Step {} shows frame {}, which the animation lacks.",
            player.step(),
            player.frame()
        )));
    };
    let Some(staged) = state.staged.as_ref() else {
        return row.child(muted("No record plays this animation, so its frame sheet is unknown."));
    };
    let Some(bounds) = animation_bounds(animation, sizes, state.mirror) else {
        return row.child(muted("The animation has no cells."));
    };

    let colours = state.colours();
    let clear = always_clear(section);
    let composite = compose(&staged.stage, frame, sizes, state.mirror, bounds);
    let pixels: Vec<[u8; 4]> = composite
        .pixels
        .iter()
        .map(|pixel| pixel.map_or(CLEAR, |pixel| pixel_colour(pixel.texel, colours, &clear)))
        .collect();
    let mut outlines = Vec::new();
    if state.outlines {
        for &cell in &frame.cells {
            outlines.push((offset(cell_rect(cell, sizes, state.mirror).0, bounds), CELL_LINE));
        }
        if let Some(box_) = frame_bounds(frame, sizes, state.mirror) {
            outlines.push((offset(box_, bounds), FRAME_LINE));
        }
    }
    let origin = (-bounds.left, -bounds.top);
    let zoom = state.zoom as f32;
    let (width, height) = (bounds.width(), bounds.height());
    let frame_canvas = canvas(
        |_, _, _| {},
        move |area, (), window, _| {
            paint_pixels(window, area, &pixels, width, zoom);
            for (rect, line) in &outlines {
                paint_rect(window, area, *rect, zoom, *line);
            }
            paint_cross(window, area, origin, zoom);
        },
    )
    .w(px(width as f32 * zoom))
    .h(px(height as f32 * zoom))
    .flex_none();

    let sheet = &section.images[staged.key.1];
    let sheet_view = sheet_canvas(sheet, section, frame, sizes, colours, &clear, state.zoom);
    let mut report = format!(
        "Frame {} of sheet {} ({}×{} texels, {}); actor position marked in red.",
        player.frame(),
        staged.key.1,
        sheet.texel_width(),
        sheet.height,
        sheet.encoding.label()
    );
    if composite.outside > 0 {
        report.push_str(&format!(
            " {} cell texels fall past the sheet: what VRAM holds there depends on the running game.",
            composite.outside
        ));
    }
    let unresolved = composite.unresolved_colours();
    if unresolved > 0 {
        report.push_str(&format!(" {unresolved} pixels use an index past the palette (marked magenta)."));
    }

    div()
        .flex()
        .flex_col()
        .gap(px(GAP_SMALL))
        .child(muted(report))
        .child(row.child(framed(frame_canvas)).child(framed(sheet_view)))
}

/// A canvas on the view background.
fn framed(child: impl IntoElement) -> Div {
    div().flex_none().p(px(GAP_SMALL)).bg(color(BACKGROUND)).child(child)
}

/// The sheet's texels in the shown colours, with the frame's cells outlined at their `u`, `v`.
fn sheet_canvas(
    sheet: &Image,
    section: &Section,
    frame: &Frame,
    sizes: &SpriteSizes,
    colours: Colours,
    clear: &[bool; COLOURS],
    zoom: u32,
) -> impl IntoElement {
    let palette = match colours {
        Colours::Palette(palette) => section.palettes.get(palette).copied(),
        Colours::Grey => None,
    };
    let pixels: Vec<[u8; 4]> =
        sheet.texels().iter().map(|&index| sheet_colour(index, palette.as_ref(), clear)).collect();
    let width = sheet.texel_width();
    let height = usize::from(sheet.height);
    let cells: Vec<Bounds> = frame
        .cells
        .iter()
        .map(|&cell| {
            let (cell_width, cell_height) = sizes.size(cell);
            let (left, top) = (i32::from(cell.u), i32::from(cell.v));
            Bounds { left, top, right: left + cell_width as i32, bottom: top + cell_height as i32 }
        })
        .collect();
    let zoom = (zoom as f32 / 2.0).max(1.0);
    canvas(
        |_, _, _| {},
        move |area, (), window, _| {
            paint_pixels(window, area, &pixels, width, zoom);
            for rect in &cells {
                paint_rect(window, area, *rect, zoom, CELL_LINE);
            }
        },
    )
    .w(px(width as f32 * zoom))
    .h(px(height as f32 * zoom))
    .flex_none()
}

/// The colour of a composed texel.
fn pixel_colour(texel: format::Texel, colours: Colours, clear: &[bool; COLOURS]) -> [u8; 4] {
    match colours {
        Colours::Grey if !texel.in_sheet || clear.get(usize::from(texel.index)).copied().unwrap_or(false) => CLEAR,
        Colours::Grey => grey(texel.index),
        Colours::Palette(_) if !texel.drawn() => CLEAR,
        Colours::Palette(_) if !texel.in_palette => MARKER,
        Colours::Palette(_) => color_rgba(texel.word),
    }
}

/// The colour of a sheet texel read straight from the sheet.
fn sheet_colour(index: u8, palette: Option<&Palette>, clear: &[bool; COLOURS]) -> [u8; 4] {
    let index = usize::from(index);
    match palette {
        Some(palette) => palette.get(index).map_or(MARKER, |&word| color_rgba(word)),
        None if clear.get(index).copied().unwrap_or(false) => CLEAR,
        None => grey(index as u8),
    }
}

/// The indices every palette of the section leaves transparent (`0x0000`), the ones grey leaves clear.
fn always_clear(section: &Section) -> [bool; COLOURS] {
    let mut clear = [true; COLOURS];
    for palette in &section.palettes {
        for (slot, &word) in clear.iter_mut().zip(palette) {
            *slot &= word == format::TRANSPARENT;
        }
    }
    clear
}

/// Index `index` of a ramp from black to white over a palette's colours.
fn grey(index: u8) -> [u8; 4] {
    let level = (usize::from(index).min(COLOURS - 1) * GREY_MAX / (COLOURS - 1)) as u8;
    [level, level, level, OPAQUE]
}

/// `rect` moved so `bounds` starts at 0, 0.
fn offset(rect: Bounds, bounds: Bounds) -> Bounds {
    Bounds {
        left: rect.left - bounds.left,
        top: rect.top - bounds.top,
        right: rect.right - bounds.left,
        bottom: rect.bottom - bounds.top,
    }
}

/// A one-pixel outline around `rect`, in texels.
fn paint_rect(window: &mut Window, area: Area<Pixels>, rect: Bounds, zoom: f32, line: u32) {
    let origin = point(area.origin.x + px(rect.left as f32 * zoom), area.origin.y + px(rect.top as f32 * zoom));
    let (width, height) = (px(rect.width() as f32 * zoom), px(rect.height() as f32 * zoom));
    let thin = px(LINE);
    for (at, extent) in [
        (origin, size(width, thin)),
        (point(origin.x, origin.y + height - thin), size(width, thin)),
        (origin, size(thin, height)),
        (point(origin.x + width - thin, origin.y), size(thin, height)),
    ] {
        window.paint_quad(fill(Area::new(at, extent), rgba(line)));
    }
}

/// A cross at the actor's position, in texels.
fn paint_cross(window: &mut Window, area: Area<Pixels>, (x, y): (i32, i32), zoom: f32) {
    let centre = point(area.origin.x + px(x as f32 * zoom), area.origin.y + px(y as f32 * zoom));
    let (arm, thin) = (px(CROSS), px(LINE));
    window.paint_quad(fill(Area::new(point(centre.x - arm, centre.y), size(arm * 2.0, thin)), rgba(ORIGIN_LINE)));
    window.paint_quad(fill(Area::new(point(centre.x, centre.y - arm), size(thin, arm * 2.0)), rgba(ORIGIN_LINE)));
}

/// The steps of the stream: each one's frame and time, or its jump. A click shows that step.
fn step_list(animation: &Animation, player: Option<Player>, cx: &mut Context<Editor>) -> Div {
    let mut list = div().flex().flex_wrap().gap(px(GAP_SMALL));
    for (index, step) in animation.steps.iter().enumerate() {
        let (label, rule) = match step.jump() {
            Some(target) => (format!("{index}: jump to {target}"), format!("delay byte {:#04X}", step.delay)),
            None => (
                format!("{index}: frame {}", step.frame),
                format!("{} ticks, {} ms", ticks_of(step.delay), duration_ms(step.delay)),
            ),
        };
        let on_screen = player.is_some_and(|player| player.step() == index);
        list = list.child(tab(("step", index), label, on_screen).tooltip(tip(rule)).on_click(cx.listener(
            move |editor, _, _, cx| {
                editor.views.sprite.clock = None;
                editor.views.sprite.seek(index);
                cx.notify();
            },
        )));
    }
    div().flex().flex_col().gap(px(GAP_SMALL)).child(strong("Steps")).child(list)
}

/// Every palette of the section as swatches, and the picker of the chosen colour.
fn palette_editor(state: &State, section: &Section, entry: &Entry<'_>, cx: &mut Context<Editor>) -> Div {
    let original = sprites::unpack(entry.original())
        .ok()
        .and_then(|(_, bytes)| format::decode(&bytes).ok())
        .and_then(|bank| bank.sections.into_iter().nth(state.section));
    let (palette, colour) = state.colour;
    let word = section.palettes.get(palette).and_then(|colours| colours.get(colour)).copied().unwrap_or_default();

    let mut rows = div().flex().flex_col().gap(px(SWATCH_GAP));
    for (number, colours) in section.palettes.iter().enumerate() {
        let mut line = div()
            .flex()
            .items_center()
            .gap(px(SWATCH_GAP))
            .child(mono(format!("palette {number}")).w(px(LIST_WIDTH / 3.0)).text_color(color(MUTED)));
        for (index, &value) in colours.iter().enumerate() {
            let stored = original.as_ref().and_then(|section| section.palettes.get(number)).map(|stored| stored[index]);
            let edited = stored.is_some_and(|stored| stored != value);
            let id = number * COLOURS + index;
            line = line.child(
                swatch(
                    ("sprite-colour", id),
                    color_rgba(value),
                    SWATCH,
                    outline((number, index) == state.colour, edited),
                )
                .tooltip(tip(format!("palette {number} colour {index}: {value:#06X}")))
                .on_click(cx.listener(move |editor, _, _, cx| {
                    editor.views.sprite.colour = (number, index);
                    cx.notify();
                })),
            );
        }
        rows = rows.child(line);
    }
    div()
        .flex()
        .flex_col()
        .gap(px(GAP))
        .child(strong("Palettes"))
        .child(muted(format!("Palette {palette} colour {colour}")))
        .child(picker(word, palette * COLOURS + colour, &state.sliders, set_colour, cx))
        .child(rows)
}

/// Starts the play clock: a timer that wakes once a tick and plays the ticks the wall clock has passed.
fn play(editor: &mut Editor, cx: &mut Context<Editor>) {
    let state = &mut editor.views.sprite;
    if state.player().is_none() {
        state.restart();
    }
    state.generation += 1;
    let generation = state.generation;
    state.clock = Some(Clock { started: Instant::now(), ticks: 0, generation });
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

/// Plays the ticks due since the clock of play `generation` started. Returns false once that play ended.
fn advance(editor: &mut Editor, generation: u64) -> bool {
    let state = &mut editor.views.sprite;
    let Some(mut clock) = state.clock.filter(|clock| clock.generation == generation) else { return false };
    let due = (clock.started.elapsed().as_secs_f64() * 1_000_000.0 / TICK_MICROS) as u64;
    for _ in 0..due.saturating_sub(clock.ticks).min(CATCH_UP_TICKS) {
        if !state.tick() {
            state.clock = None;
            return false;
        }
    }
    clock.ticks = due;
    let held = state.playback == Playback::Once && state.player().is_some_and(|player| player.ended());
    state.clock = (!held).then_some(clock);
    !held
}

/// Rewrites colour `index` (palette `index / 32`, colour `index % 32`) of the shown section.
fn set_colour(editor: &mut Editor, index: usize, change: &dyn Fn(u16) -> u16) {
    let section = editor.views.sprite.section;
    let Some(entry) = editor.current() else { return };
    match with_colour(entry.payload(), section, index / COLOURS, index % COLOURS, change) {
        Ok(payload) => editor.set_payload(payload),
        Err(error) => editor.status = error,
    }
}

/// The stored payload with colour `colour` of palette `palette` of section `section` changed by `change`.
pub fn with_colour(
    payload: &[u8],
    section: usize,
    palette: usize,
    colour: usize,
    change: &dyn Fn(u16) -> u16,
) -> Result<Vec<u8>, String> {
    let (_, bytes) = sprites::unpack(payload).map_err(|error| error.to_string())?;
    let mut bank = format::decode(&bytes).map_err(|error| error.to_string())?;
    let word = bank
        .sections
        .get_mut(section)
        .and_then(|section| section.palettes.get_mut(palette))
        .and_then(|colours| colours.get_mut(colour))
        .ok_or("the colour is not in the bank")?;
    *word = change(*word);
    let encoded = format::encode(&bank).map_err(|error| error.to_string())?;
    sprites::pack(payload, &encoded).map_err(|error| error.to_string())
}

/// The file name an export of the shown section starts from.
fn sheet_name(entry: &Entry<'_>, section: usize) -> String {
    let (_, file) = entry.archive.path.split();
    let stem = file.split_once('.').map_or(file, |(stem, _)| stem);
    format!("{stem}-{}-section{section}.json", entry.index)
}

/// Asks where to write the shown section's sprite sheet, then writes the JSON there and the PNG beside it.
fn export_sheet(editor: &mut Editor, cx: &mut Context<Editor>) {
    let state = &editor.views.sprite;
    let (Some(sizes), Colours::Palette(palette)) = (state.sizes().copied(), state.colours()) else { return };
    let section = state.section;
    let Some(entry) = editor.current() else { return };
    let payload = entry.payload().to_vec();
    let name = sheet_name(&entry, section);
    let folder = editor.project.as_ref().map_or_else(|| PathBuf::from("."), |project| project.folder.clone());
    let picked = cx.prompt_for_new_path(&folder, Some(&name));
    cx.spawn(async move |this, cx| {
        let Ok(Ok(Some(json_path))) = picked.await else { return };
        let status = write_sheet(&json_path, &payload, section, palette, &sizes);
        let _ = this.update(cx, |editor, cx| {
            editor.status = status;
            cx.notify();
        });
    })
    .detach();
}

/// Writes the sheet's JSON to `json_path` and its PNG next to it. Returns the status line.
fn write_sheet(json_path: &Path, payload: &[u8], section: usize, palette: usize, sizes: &SpriteSizes) -> String {
    let png_path = json_path.with_extension("png");
    let image = png_path.file_name().map_or("sheet.png".into(), |name| name.to_string_lossy().into_owned());
    let written = sprites::sprite_sheet(payload, section, palette, sizes, &image)
        .map_err(|error| error.to_string())
        .and_then(|sheet| {
            std::fs::write(&png_path, &sheet.png).map_err(|error| format!("{}: {error}", png_path.display()))?;
            std::fs::write(json_path, &sheet.json).map_err(|error| format!("{}: {error}", json_path.display()))
        });
    match written {
        Ok(()) => format!("Wrote {} and {}", json_path.display(), png_path.display()),
        Err(error) => format!("Export: {error}"),
    }
}

/// Asks for a sprite sheet's JSON, reads the PNG it names from the same folder, and puts the sheet back
/// into the shown entry.
fn import_sheet(editor: &mut Editor, cx: &mut Context<Editor>) {
    let Some(sizes) = editor.views.sprite.sizes().copied() else { return };
    let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: Some("Sprite sheet JSON to import".into()),
    });
    cx.spawn(async move |this, cx| {
        let Ok(Ok(Some(paths))) = picked.await else { return };
        let Some(json_path) = paths.into_iter().next() else { return };
        let files = read_sheet(&json_path);
        let _ = this.update(cx, |editor, cx| {
            editor.status = match files.and_then(|(json, png)| import_into(editor, &json, &png, &sizes)) {
                Ok(()) => format!("Imported {}", json_path.display()),
                Err(error) => format!("Import: {error}"),
            };
            cx.notify();
        });
    })
    .detach();
}

/// A sheet's JSON and the PNG it names.
fn read_sheet(json_path: &Path) -> Result<(Vec<u8>, Vec<u8>), String> {
    let json = std::fs::read(json_path).map_err(|error| format!("{}: {error}", json_path.display()))?;
    let image = sprites::image_name(&json).map_err(|error| error.to_string())?;
    let png_path = json_path.parent().unwrap_or(Path::new(".")).join(image);
    let png = std::fs::read(&png_path).map_err(|error| format!("{}: {error}", png_path.display()))?;
    Ok((json, png))
}

/// Reads a sheet back into the shown entry.
fn import_into(editor: &mut Editor, json: &[u8], png: &[u8], sizes: &SpriteSizes) -> Result<(), String> {
    let entry = editor.current().ok_or("no entry is shown")?;
    let payload = sprites::import_sprite_sheet(json, png, entry.payload(), sizes).map_err(|error| error.to_string())?;
    editor.set_payload(payload);
    Ok(())
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;
    use rebof3_emi::lz77;
    use rebof3_emi::sprite::{Cell, Encoding, SHAPES, Step};

    use super::*;
    use crate::project::ArchivePath;
    use crate::view::Tab;

    /// A bare section: one sheet, one palette, one animation of two frames, one record, and padding.
    fn section() -> Vec<u8> {
        let texels: Vec<u8> = (0..16 * 8).map(|index| (index % 31 + 1) as u8).collect();
        let sheet = Image::new(8, 8, Encoding::Raw, texels).expect("a sheet").encode();
        let cell = Cell { size: 1, x: -8, y: -4, u: 0, v: 0 };
        let animation = Animation {
            steps: vec![Step { delay: 3, frame: 0 }, Step { delay: 2, frame: 1 }],
            frames: vec![Frame { cells: vec![cell] }, Frame { cells: vec![Cell { u: 8, ..cell }] }],
        }
        .encode();
        let mut out = vec![0u8; 16];
        let images = out.len();
        out.extend(4u32.to_le_bytes());
        out.extend(&sheet);
        let palettes = out.len();
        out.extend(4u32.to_le_bytes());
        out.extend((0..32u16).flat_map(|colour| (colour * 0x0421).to_le_bytes()));
        let animations = out.len();
        out.extend(2u16.to_le_bytes());
        out.extend(&animation);
        let records = out.len();
        out.extend([0, 0, 0, 0xFF]);
        out.resize(out.len() + 0x200, 0);
        for (index, at) in [images, palettes, animations, records].into_iter().enumerate() {
            out[4 * index..4 * index + 4].copy_from_slice(&(at as u32).to_le_bytes());
        }
        out
    }

    fn sizes() -> SpriteSizes {
        let mut rows = [[0; 2]; SHAPES];
        for (shape, row) in rows.iter_mut().enumerate() {
            *row = [8 * (shape as u8 % 4 + 1), 8 * (shape as u8 / 4 + 1)];
        }
        SpriteSizes::from_rows(rows)
    }

    fn state(bank: &[u8]) -> State {
        let bank = format::decode(bank).map_err(|error| error.to_string());
        let loaded = Loaded { key: (String::new(), 0, 0), bank, reader: Reader::Field };
        let mut state = State { loaded: Some(Rc::new(loaded)), ..State::default() };
        state.restart();
        state
    }

    #[test]
    fn a_step_plays_the_ticks_of_its_delay() {
        let mut state = state(&section());
        state.step();
        let player = state.player().expect("a player");
        assert_eq!((player.step(), player.frame(), player.ticks()), (1, 1, 3));
        state.seek(0);
        assert_eq!(state.player().map(|player| player.step()), Some(0));
    }

    #[test]
    fn a_colour_edit_survives_the_lz77_round_trip() {
        let bank = section();
        let raw = with_colour(&bank, 0, 0, 3, &|_| 0x7FFF).expect("edits");
        assert_eq!(format::decode(&raw).expect("decodes").sections[0].palettes[0][3], 0x7FFF);
        assert_eq!(raw.len(), bank.len());

        let packed = with_colour(&lz77::encode(&bank), 0, 0, 3, &|_| 0x7FFF).expect("edits");
        assert_eq!(lz77::decode(&packed).expect("decodes"), raw);
        assert!(with_colour(&bank, 0, 1, 0, &|word| word).is_err(), "palette 1 is not in the bank");
    }

    #[test]
    fn grey_clears_only_what_every_palette_clears() {
        let section = format::decode(&section()).expect("decodes").sections.remove(0);
        let clear = always_clear(&section);
        assert!(clear[0] && !clear[1]);
        let texel = format::Texel { index: 0, word: 0, in_sheet: true, in_palette: true };
        assert_eq!(pixel_colour(texel, Colours::Grey, &clear), CLEAR);
        assert_eq!(pixel_colour(format::Texel { index: 31, ..texel }, Colours::Grey, &clear), [255, 255, 255, 255]);
        let outside = format::Texel { index: 40, word: 0x1234, in_sheet: true, in_palette: false };
        assert_eq!(pixel_colour(outside, Colours::Palette(0), &clear), MARKER);
    }

    #[test]
    fn an_exported_sheet_imports_through_the_files() {
        let folder = std::env::temp_dir().join(format!("bof3-edit-sprite-sheet-{}", std::process::id()));
        std::fs::create_dir_all(&folder).expect("folder");
        let json_path = folder.join("bank.json");
        let packed = lz77::encode(&section());
        assert!(write_sheet(&json_path, &packed, 0, 0, &sizes()).starts_with("Wrote"));
        let (json, png) = read_sheet(&json_path).expect("reads back");
        assert_eq!(sprites::import_sprite_sheet(&json, &png, &packed, &sizes()).expect("imports"), packed);
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// A retail party bank draws, plays and steps in the view. Point `BOF3_DISC_DIR` at an extracted
    /// disc; without it the test skips.
    #[gpui::test]
    fn a_retail_party_bank_plays(cx: &mut TestAppContext) {
        let Some(disc) = std::env::var_os("BOF3_DISC_DIR").map(PathBuf::from) else {
            return eprintln!("skipping: set BOF3_DISC_DIR");
        };
        let folder = std::env::temp_dir().join(format!("bof3-edit-sprite-{}", std::process::id()));
        let project = folder.clone();
        let (editor, cx) = cx.add_window_view(|_, cx| Editor::new(Some(disc), Some(project), cx));
        editor.update(cx, |editor, _| {
            editor.select_archive(&ArchivePath("BIN/PLCHAR/PL012.EMI".into()));
            let formats = &editor.archive.as_ref().expect("the archive loads").formats;
            let index = formats.iter().position(|format| matches!(format, rebof3_emi::classify::Format::Lz77Blob));
            editor.select_entry(index.expect("the archive has a sprite bank"));
            editor.tab = Tab::Sprite;
        });
        cx.update(|window, cx| window.draw(cx).clear());

        let (before, after) = editor.update(cx, |editor, _| {
            let state = &mut editor.views.sprite;
            assert_eq!(state.colours(), Colours::Palette(format::FIELD_PALETTE), "a party bank's palette is proven");
            assert!(state.staged.is_some(), "the first animation's sheet is staged");
            let before = state.summary().expect("the view shows a frame");
            state.step();
            (before, state.summary().expect("still shows a frame"))
        });
        assert!(before.starts_with("section 0 of 3") && before.ends_with("tick 0"), "{before}");
        assert_ne!(before, after);
        editor.update(cx, |editor, _| editor.views.sprite.section = 2);
        cx.update(|window, cx| window.draw(cx).clear());
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// Every retail sprite bank takes a colour edit and keeps every other byte of the decoded bank. Point
    /// `BOF3_DISC_DIR` at an extracted disc; without it the test skips.
    #[test]
    fn every_retail_bank_takes_a_colour_edit() {
        let Some(disc) = std::env::var_os("BOF3_DISC_DIR") else { return eprintln!("skipping: set BOF3_DISC_DIR") };
        let folder = std::env::temp_dir().join(format!("bof3-edit-sprite-colour-{}", std::process::id()));
        let mut project = crate::project::Project::open(disc.as_ref(), &folder).expect("project opens");

        let (mut banks, mut failures) = (0, Vec::new());
        for path in project.archives.clone() {
            let archive = project.load(&path).expect("archive loads");
            for index in 0..archive.entries().len() {
                let payload = archive.original(index);
                let Ok((_, bank)) = sprites::unpack(payload) else { continue };
                banks += 1;

                let edited = with_colour(payload, 0, 0, 1, &|word| word ^ 1)
                    .and_then(|bytes| sprites::unpack(&bytes).map_err(|error| error.to_string()));
                let decoded = format::decode(&bank).expect("unpacked banks decode");
                let at = bank.len() - format::encode(&decoded).expect("encodes").len()
                    + palette_offset(&bank).expect("a palette");
                let mut expected = bank.clone();
                expected[at] ^= 1;
                if edited.map(|(_, bytes)| bytes).as_ref() != Ok(&expected) {
                    failures.push(format!("{}#{index}", path.0));
                }
            }
        }

        let _ = std::fs::remove_dir_all(&folder);
        eprintln!("sprite banks: {} of {banks} fail a colour edit", failures.len());
        assert!(banks > 0 && failures.is_empty(), "{:?}", failures.iter().take(8).collect::<Vec<_>>());
    }

    /// The bank offset of colour 1 of palette 0 of section 0: the section's palette table offset, then
    /// the palette's offset from the table.
    fn palette_offset(bank: &[u8]) -> Option<usize> {
        let word = |at: usize| Some(u32::from_le_bytes(bank.get(at..at + 4)?.try_into().ok()?) as usize);
        let section = if word(0)? == format::CONTAINER_HEAD as usize { word(0)? } else { 0 };
        let table = section + word(section + 4)?;
        Some(table + word(table)? + 2)
    }
}
