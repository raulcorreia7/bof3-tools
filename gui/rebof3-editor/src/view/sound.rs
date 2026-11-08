//! Sound banks and sequences: play a VAB's samples, list its programs and tones, export WAVE,
//! SoundFont and MIDI files, edit a sequence's header and import a sequence from MIDI.

use gpui::{AnyElement, Context, Div, div, prelude::*, px};
use psx_formats::sep::{Package, Sequence};
use psx_formats::vab::Bank;
use rebof3_assets::decode::{pcm_wav, sequence_midi, soundfont};
use rebof3_assets::sound::sequence_pcm;
use rebof3_emi::classify::Format;
use rebof3_emi::entry::vab_body_index;

use super::{Entry, Field, input};
use crate::app::Editor;
use crate::audio::{Player, to_mono};
use crate::fonts;
use crate::theme::{RAISED, color};
use crate::widgets::{GAP, GAP_SECTION, GAP_SMALL, ROW_HEIGHT, button, mono, muted, separator, strong};

/// Width of a sample or tone column, in pixels.
const COLUMN_WIDTH: f32 = 80.0;
/// Most height the tone list takes before it scrolls, in pixels.
const TONES_HEIGHT: f32 = 320.0;
/// How much of a sequence the Play button renders, in seconds; a looping song never ends.
const SEQUENCE_PREVIEW_SECONDS: f64 = 90.0;
/// Width of a sequence header field, in pixels.
const HEADER_FIELD_WIDTH: f32 = 72.0;
/// The largest tempo the header's three bytes hold, in microseconds per quarter note.
const TEMPO_LIMIT: u32 = 0xFF_FFFF;
/// The largest beat-note exponent the header stores: a 128th note.
const BEAT_NOTE_EXPONENT_LIMIT: u32 = 7;
/// Microseconds in a minute, to show a tempo as beats per minute.
const MICROSECONDS_PER_MINUTE: f64 = 60_000_000.0;

/// A sequence header field (`wiki: formats/audio`): what the sequencer starts the sequence with.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Part {
    /// Ticks per quarter note.
    Resolution,
    /// Microseconds per quarter note.
    Tempo,
    /// The time signature's numerator.
    Beats,
    /// The time signature's denominator, stored as its power-of-two exponent.
    BeatNote,
}

impl Part {
    const ALL: [Part; 4] = [Part::Resolution, Part::Tempo, Part::Beats, Part::BeatNote];

    fn label(self) -> &'static str {
        match self {
            Part::Resolution => "ticks per beat",
            Part::Tempo => "µs per beat",
            Part::Beats => "beats",
            Part::BeatNote => "beat note",
        }
    }

    /// What the field accepts, for the status line.
    fn rule(self) -> &'static str {
        match self {
            Part::Resolution => "Ticks per quarter note is a number from 1 to 65535.",
            Part::Tempo => "A tempo is 1 to 16777215 microseconds per quarter note.",
            Part::Beats => "The beats of a bar are a number from 1 to 255.",
            Part::BeatNote => "The beat note is 1, 2, 4, 8, 16, 32, 64 or 128.",
        }
    }

    /// The field's text for `sequence`.
    fn text(self, sequence: &Sequence) -> String {
        match self {
            Part::Resolution => sequence.resolution.to_string(),
            Part::Tempo => sequence.tempo_us.to_string(),
            Part::Beats => sequence.time_num.to_string(),
            Part::BeatNote => match 1u32.checked_shl(u32::from(sequence.time_den)) {
                Some(note) if u32::from(sequence.time_den) <= BEAT_NOTE_EXPONENT_LIMIT => note.to_string(),
                _ => format!("2^{}", sequence.time_den),
            },
        }
    }

    /// Sets the field of `sequence` from `text`, or says what the field accepts.
    fn set(self, sequence: &mut Sequence, text: &str) -> Result<(), String> {
        let number: u32 = text.trim().parse().map_err(|_| self.rule().to_string())?;
        let valid = match self {
            Part::Resolution => u16::try_from(number).ok().filter(|&ticks| ticks > 0).map(|ticks| {
                sequence.resolution = ticks;
            }),
            Part::Tempo => (1..=TEMPO_LIMIT).contains(&number).then(|| sequence.tempo_us = number),
            Part::Beats => u8::try_from(number).ok().filter(|&beats| beats > 0).map(|beats| {
                sequence.time_num = beats;
            }),
            Part::BeatNote => (number.is_power_of_two() && number.trailing_zeros() <= BEAT_NOTE_EXPONENT_LIMIT)
                .then(|| sequence.time_den = number.trailing_zeros() as u8),
        };
        valid.ok_or_else(|| self.rule().to_string())
    }
}

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
        return sequences(editor, entry, cx);
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

/// Renders the sequence with id `sequence` of the shown archive, edits included, and plays it.
fn play_sequence(editor: &mut Editor, sequence: u16) -> Result<(), String> {
    let entry = editor.current().ok_or("no entry")?;
    let archive = entry.project.current_archive(entry.archive)?;
    let path = entry.archive.path.0.clone();
    let pcm = sequence_pcm(&path, &archive, u32::from(sequence), SEQUENCE_PREVIEW_SECONDS)
        .map_err(|error| error.to_string())?;
    editor.views.sound.player.play(to_mono(&pcm.samples, pcm.sample_rate, pcm.channels))
}

/// A tempo in microseconds per quarter note, as beats per minute.
fn bpm(tempo_us: u32) -> String {
    if tempo_us == 0 { "-".into() } else { format!("{:.1}", MICROSECONDS_PER_MINUTE / f64::from(tempo_us)) }
}

/// The package with header field `part` of sequence `index` set from `text`.
fn with_header(payload: &[u8], index: usize, part: Part, text: &str) -> Result<Vec<u8>, String> {
    let mut package = Package::parse(payload).map_err(|error| error.to_string())?;
    let sequence = package.sequences.get_mut(index).ok_or("the package has no such sequence")?;
    part.set(sequence, text)?;
    Ok(package.encode())
}

/// The package with sequence `index` replaced by the format 0 Standard MIDI File `midi`. The sequence
/// keeps its id, which MIDI does not carry.
fn with_midi(payload: &[u8], index: usize, midi: &[u8]) -> Result<Vec<u8>, String> {
    let mut package = Package::parse(payload).map_err(|error| error.to_string())?;
    let sequence = package.sequences.get_mut(index).ok_or("the package has no such sequence")?;
    *sequence = Sequence::from_midi(midi, sequence.seq_id).map_err(|error| format!("The MIDI file: {error}"))?;
    Ok(package.encode())
}

/// Applies a changed header field of the shown package. Returns a status message when the text cannot be
/// written.
pub fn header_changed(editor: &mut Editor, index: usize, part: Part, text: &str) -> Option<String> {
    let entry = editor.current()?;
    match with_header(entry.payload(), index, part, text) {
        Ok(payload) => {
            editor.set_payload(payload);
            None
        }
        Err(error) => Some(error),
    }
}

/// Asks for a MIDI file and puts it in place of sequence `index` of the shown package.
fn import_midi(editor: &mut Editor, index: usize, cx: &mut Context<Editor>) {
    let Some(entry) = editor.current() else { return };
    let (archive, entry_index) = (entry.archive.path.clone(), entry.index);
    editor.open_file("MIDI file to import", cx, move |editor, midi| {
        match editor.rewrite_entry(&archive, entry_index, |payload| with_midi(payload, index, &midi)) {
            Ok(()) => format!("Imported sequence {index} from MIDI."),
            Err(error) => format!("Import: {error}"),
        }
    });
}

fn sequences(editor: &Editor, entry: &Entry<'_>, cx: &mut Context<Editor>) -> AnyElement {
    let payload = entry.payload().to_vec();
    let package = match Package::parse(&payload) {
        Ok(package) => package,
        Err(error) => return muted(format!("The sequence package does not parse: {error}")).into_any_element(),
    };
    let title = format!("{}-{}", stem(entry), entry.index);

    let mut top =
        div().flex().items_center().gap(px(GAP)).child(strong(format!("{} sequences", package.sequences.len())));
    if editor.views.sound.player.is_playing() {
        top = top.child(button("stop", "Stop", true).on_click(cx.listener(|editor, _, _, cx| {
            editor.views.sound.player.stop();
            cx.notify();
        })));
    }
    let mut headings = vec![strong("sequence").into_any_element(), strong("id").into_any_element()];
    headings.extend(Part::ALL.map(|part| strong(part.label()).into_any_element()));
    headings.push(strong("BPM").into_any_element());
    let mut list = div().flex().flex_col().gap(px(GAP_SMALL)).child(top).child(grid_row(headings, false));
    for (index, sequence) in package.sequences.iter().enumerate() {
        let (payload, name) = (payload.clone(), format!("{title}-{index}.mid"));
        let midi = button(("midi", index), "MIDI…", true).on_click(cx.listener(move |editor, _, _, cx| {
            let bytes = sequence_midi(&payload, index).map_err(|error| error.to_string());
            editor.save_file(name.clone(), bytes, cx);
        }));
        let id = sequence.seq_id;
        let play = button(("play-sequence", index), "▶ Play", sequence.has_notes()).on_click(cx.listener(
            move |editor, _, _, cx| {
                if let Err(error) = play_sequence(editor, id) {
                    editor.status = format!("Play: {error}");
                }
                cx.notify();
            },
        ));
        let import =
            button(("import-midi", index), "Import MIDI…", true).on_click(cx.listener(move |editor, _, _, cx| {
                import_midi(editor, index, cx);
                cx.notify();
            }));

        let mut cells = vec![text(index), text(id)];
        for part in Part::ALL {
            let which = Field::SequenceHeader { index, part };
            let id = ("sequence-header", index * Part::ALL.len() + part as usize);
            cells.push(
                input(editor, id, which, &part.text(sequence), "", Some(HEADER_FIELD_WIDTH), cx).into_any_element(),
            );
        }
        cells.extend([
            text(bpm(sequence.tempo_us)),
            text(if sequence.has_notes() { "notes" } else { "silent" }),
            play.into_any_element(),
            midi.into_any_element(),
            import.into_any_element(),
        ]);
        list = list.child(grid_row(cells, index % 2 == 1));
    }
    list.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use psx_formats::sep::Event;

    /// The end-of-track meta event.
    const END_OF_TRACK: u8 = 0x2F;
    /// The note-on status of channel 0.
    const NOTE_ON: u8 = 0x90;

    /// A package of one sequence: one note, then the end of the track, at 120 BPM in 4/4.
    fn package() -> Vec<u8> {
        let note = Event { status: NOTE_ON, data1: 60, data2: 100, status_byte: true, ..Event::default() };
        let end = Event { status: 0xFF, meta_type: END_OF_TRACK, status_byte: true, ..Event::default() };
        let sequence = Sequence {
            seq_id: 3,
            resolution: 48,
            tempo_us: 500_000,
            time_num: 4,
            time_den: 2,
            events: vec![note, end],
        };
        Package { sequences: vec![sequence] }.encode()
    }

    #[test]
    fn a_header_edit_changes_only_its_field() {
        let payload = package();
        let edited = with_header(&payload, 0, Part::Tempo, "400000").unwrap();
        let sequence = &Package::parse(&edited).unwrap().sequences[0];
        assert_eq!((sequence.tempo_us, sequence.resolution, sequence.seq_id), (400_000, 48, 3));

        let changed: Vec<usize> = (0..payload.len()).filter(|&at| payload[at] != edited[at]).collect();
        assert!(changed.iter().all(|at| (10..13).contains(at)), "only the tempo bytes change: {changed:?}");
    }

    #[test]
    fn a_beat_note_is_stored_as_its_exponent() {
        let edited = with_header(&package(), 0, Part::BeatNote, "8").unwrap();
        let sequence = &Package::parse(&edited).unwrap().sequences[0];
        assert_eq!(sequence.time_den, 3);
        assert_eq!(Part::BeatNote.text(sequence), "8");
    }

    #[test]
    fn a_header_value_out_of_range_is_refused() {
        let payload = package();
        for (part, text) in [
            (Part::Resolution, "0"),
            (Part::Resolution, "65536"),
            (Part::Tempo, "16777216"),
            (Part::Beats, "256"),
            (Part::BeatNote, "6"),
            (Part::BeatNote, "256"),
            (Part::Tempo, "fast"),
        ] {
            assert_eq!(with_header(&payload, 0, part, text), Err(part.rule().to_string()), "{part:?} {text}");
        }
        assert!(with_header(&payload, 1, Part::Tempo, "1").is_err(), "no second sequence");
    }

    #[test]
    fn an_exported_sequence_imports_back_exactly() {
        let payload = package();
        let midi = Package::parse(&payload).unwrap().to_midi(0).unwrap();
        assert_eq!(with_midi(&payload, 0, &midi).unwrap(), payload);
        assert!(with_midi(&payload, 0, b"not a MIDI file").is_err());
    }

    /// Every sequence of every retail package exports to MIDI and imports back to the stored bytes. Point
    /// `BOF3_DISC_DIR` at an extracted disc; without it the test skips.
    #[test]
    fn every_retail_sequence_imports_back_from_its_midi() {
        let Some(disc) = std::env::var_os("BOF3_DISC_DIR") else { return eprintln!("skipping: set BOF3_DISC_DIR") };
        let folder = std::env::temp_dir().join(format!("bof3-edit-midi-{}", std::process::id()));
        let mut project = crate::project::Project::open(disc.as_ref(), &folder).expect("project opens");

        let (mut sequences, mut failures) = (0, Vec::new());
        for path in project.archives.clone() {
            let archive = project.load(&path).expect("archive loads");
            for index in (0..archive.entries().len()).filter(|&index| archive.formats[index] == Format::SepPackage) {
                let payload = archive.original(index);
                let package = Package::parse(payload).expect("a retail package parses");
                for sequence in 0..package.sequences.len() {
                    sequences += 1;
                    let back = package
                        .to_midi(sequence)
                        .map_err(|error| error.to_string())
                        .and_then(|midi| with_midi(payload, sequence, &midi));
                    if back.as_deref() != Ok(payload) {
                        failures.push(format!("{}#{index} sequence {sequence}", path.0));
                    }
                }
            }
        }

        let _ = std::fs::remove_dir_all(&folder);
        eprintln!("midi: {} of {sequences} sequences do not import back", failures.len());
        assert!(sequences > 0 && failures.is_empty(), "{:?}", failures.iter().take(8).collect::<Vec<_>>());
    }
}
