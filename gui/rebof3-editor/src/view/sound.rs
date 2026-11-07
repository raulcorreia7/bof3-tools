//! Sound banks and sequences: play a VAB's samples, list its programs and tones, export WAVE,
//! SoundFont and MIDI files.

use gpui::{AnyElement, Context, Div, div, prelude::*, px};
use psx_formats::sep::Package;
use psx_formats::vab::Bank;
use rebof3_assets::decode::{pcm_wav, sequence_midi, soundfont};
use rebof3_emi::classify::Format;
use rebof3_emi::entry::vab_body_index;

use super::Entry;
use crate::app::Editor;
use crate::audio::Player;
use crate::fonts;
use crate::theme::{RAISED, color};
use crate::widgets::{GAP, GAP_SECTION, GAP_SMALL, ROW_HEIGHT, button, mono, muted, separator, strong};

/// Width of a sample or tone column, in pixels.
const COLUMN_WIDTH: f32 = 80.0;
/// Most height the tone list takes before it scrolls, in pixels.
const TONES_HEIGHT: f32 = 320.0;

#[derive(Default)]
pub struct State {
    pub player: Player,
    /// Whether a clip played at the last tick, so the window redraws when it ends.
    was_playing: bool,
    tones_open: bool,
}

impl State {
    /// Returns true when the playing state changed since the last tick.
    pub fn tick(&mut self) -> bool {
        let playing = self.player.is_playing();
        let changed = playing != self.was_playing;
        self.was_playing = playing;
        changed
    }
}

/// The header and body entries of the VAB an entry belongs to.
fn vab_pair(entry: &Entry<'_>) -> Option<(usize, usize)> {
    let entries = entry.archive.entries();
    match entry.format() {
        Format::VabHeader => Some((entry.index, vab_body_index(entries, entry.index).ok()?)),
        Format::VabBody => (0..entries.len())
            .filter(|&header| entry.archive.formats[header] == Format::VabHeader)
            .find(|&header| vab_body_index(entries, header).ok() == Some(entry.index))
            .map(|header| (header, entry.index)),
        _ => None,
    }
}

/// The archive's file name without `.EMI`, the stem of every exported file.
fn stem(entry: &Entry<'_>) -> String {
    entry.archive.path.split().1.trim_end_matches(".EMI").to_string()
}

/// A row of fixed-width cells.
fn grid_row(cells: impl IntoIterator<Item = AnyElement>, striped: bool) -> Div {
    let mut row =
        div().flex().items_center().gap(px(GAP)).min_h(px(ROW_HEIGHT)).when(striped, |row| row.bg(color(RAISED)));
    for cell in cells {
        row = row.child(div().w(px(COLUMN_WIDTH)).flex_none().child(cell));
    }
    row
}

fn text(value: impl ToString) -> AnyElement {
    div().child(value.to_string()).into_any_element()
}

pub fn show(editor: &Editor, entry: &Entry<'_>, cx: &mut Context<Editor>) -> AnyElement {
    if entry.format() == Format::SepPackage {
        return sequences(entry, cx);
    }
    let Some((header_index, body_index)) = vab_pair(entry) else {
        return muted("No matching VAB header and body in this archive.").into_any_element();
    };
    let header = entry.sibling(header_index);
    let body = entry.sibling(body_index);
    let bank = match Bank::parse(header, body) {
        Ok(bank) => bank,
        Err(error) => return muted(format!("The VAB does not parse: {error}")).into_any_element(),
    };
    let title = format!("{}-{header_index}", stem(entry));
    let state = &editor.views.sound;

    let mut top = div().flex().flex_none().items_center().gap(px(GAP)).child(mono(format!(
        "VAB id {}  {} programs  {} tones  {} samples  volume {}  pan {}",
        bank.id, bank.program_count, bank.tone_count, bank.vag_count, bank.master_vol, bank.master_pan
    )));
    let (soundfont_header, soundfont_body, soundfont_title) = (header.to_vec(), body.to_vec(), title.clone());
    top = top.child(button("soundfont", "Export SoundFont…", true).on_click(cx.listener(move |editor, _, _, cx| {
        let bytes = soundfont(&soundfont_header, &soundfont_body, &soundfont_title).map_err(|error| error.to_string());
        editor.save_file(format!("{soundfont_title}.sf2"), bytes, cx);
    })));
    if state.player.is_playing() {
        top = top.child(button("stop", "Stop", true).on_click(cx.listener(|editor, _, _, cx| {
            editor.views.sound.player.stop();
            cx.notify();
        })));
    }

    let mut samples = div().flex().flex_col().font_family(fonts::mono());
    samples = samples
        .child(grid_row(["#", "bytes", "frames", "loop"].map(|heading| strong(heading).into_any_element()), false));
    for (at, vag) in bank.vags.iter().filter(|vag| !vag.is_empty()).enumerate() {
        let looped =
            if vag.has_loop { format!("{}..{}", vag.loop_start_frame, vag.loop_end_frame) } else { "-".into() };
        let pcm = bank.decode_vag(body, vag.index).map_err(|error| error.to_string());
        let (play_pcm, wave_pcm, wave_name) = (pcm.clone(), pcm, format!("{title}-{}.wav", vag.index));

        let play = button(("play", vag.index), "▶ Play", true).on_click(cx.listener(move |editor, _, _, cx| {
            if let Err(error) = play_pcm.clone().and_then(|pcm| editor.views.sound.player.play(pcm)) {
                editor.status = format!("Play: {error}");
            }
            cx.notify();
        }));
        let wave = button(("wave", vag.index), "WAVE…", true).on_click(cx.listener(move |editor, _, _, cx| {
            let bytes = wave_pcm.clone().and_then(|pcm| pcm_wav(&pcm).map_err(|error| error.to_string()));
            editor.save_file(wave_name.clone(), bytes, cx);
        }));
        let cells = [
            text(vag.index),
            text(vag.size),
            text(vag.frames),
            text(looped),
            play.into_any_element(),
            wave.into_any_element(),
        ];
        samples = samples.child(grid_row(cells, at % 2 == 1));
    }

    let arrow = if state.tones_open { "▾" } else { "▸" };
    let tones_title =
        div().id("tones").cursor_pointer().child(strong(format!("{arrow} Tones ({})", bank.tones.len()))).on_click(
            cx.listener(|editor, _, _, cx| {
                editor.views.sound.tones_open = !editor.views.sound.tones_open;
                cx.notify();
            }),
        );
    let mut tones =
        div().id("tone-list").flex().flex_col().max_h(px(TONES_HEIGHT)).overflow_y_scroll().font_family(fonts::mono());
    if state.tones_open {
        let headings = ["program", "sample", "notes", "center", "volume", "pan", "mode"];
        tones = tones.child(grid_row(headings.map(|heading| strong(heading).into_any_element()), false));
        for (at, tone) in bank.usable_tones().enumerate() {
            let cells = [
                text(tone.program),
                text(tone.vag),
                text(format!("{}..{}", tone.min_note, tone.max_note)),
                text(tone.center_note),
                text(tone.vol),
                text(tone.pan),
                text(tone.mode),
            ];
            tones = tones.child(grid_row(cells, at % 2 == 1));
        }
    }

    div()
        .id("sound")
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .gap(px(GAP_SMALL))
        .overflow_y_scroll()
        .child(top)
        .child(separator(false))
        .child(strong("Samples"))
        .child(samples)
        .child(div().h(px(GAP_SECTION)))
        .child(tones_title)
        .child(tones)
        .into_any_element()
}

fn sequences(entry: &Entry<'_>, cx: &mut Context<Editor>) -> AnyElement {
    let payload = entry.payload().to_vec();
    let package = match Package::parse(&payload) {
        Ok(package) => package,
        Err(error) => return muted(format!("The sequence package does not parse: {error}")).into_any_element(),
    };
    let title = format!("{}-{}", stem(entry), entry.index);

    let mut list = div().flex().flex_col().child(strong(format!("{} sequences", package.sequences.len())));
    for (index, sequence) in package.sequences.iter().enumerate() {
        let (payload, name) = (payload.clone(), format!("{title}-{index}.mid"));
        let midi = button(("midi", index), "MIDI…", true).on_click(cx.listener(move |editor, _, _, cx| {
            let bytes = sequence_midi(&payload, index).map_err(|error| error.to_string());
            editor.save_file(name.clone(), bytes, cx);
        }));
        let cells = [
            text(format!("sequence {index}")),
            text(if sequence.has_notes() { "notes" } else { "silent" }),
            midi.into_any_element(),
        ];
        list = list.child(grid_row(cells, index % 2 == 1));
    }
    list.into_any_element()
}
