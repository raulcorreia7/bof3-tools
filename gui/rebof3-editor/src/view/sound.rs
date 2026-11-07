//! Sound banks and sequences: play a VAB's samples, list its programs and tones, export WAVE, SoundFont
//! and MIDI files.

use eframe::egui::{self, RichText};
use psx_formats::sep::Package;
use psx_formats::vab::Bank;
use rebof3_assets::decode::{pcm_wav, sequence_midi, soundfont};
use rebof3_emi::classify::Format;
use rebof3_emi::entry::vab_body_index;

use super::{Entry, style};
use crate::audio::Player;

#[derive(Default)]
pub struct State {
    player: Player,
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

/// Asks where to save a file and writes it. Returns the status line.
fn save(name: &str, bytes: Result<Vec<u8>, String>) -> Option<String> {
    let bytes = match bytes {
        Ok(bytes) => bytes,
        Err(error) => return Some(error),
    };
    let path = rfd::FileDialog::new().set_file_name(name).save_file()?;
    Some(match std::fs::write(&path, bytes) {
        Ok(()) => format!("Wrote {}", path.display()),
        Err(error) => format!("{}: {error}", path.display()),
    })
}

pub fn show(ui: &mut egui::Ui, entry: &mut Entry<'_>, state: &mut State) -> Option<String> {
    if entry.format() == Format::SepPackage {
        return sequences(ui, entry);
    }
    let Some((header_index, body_index)) = vab_pair(entry) else {
        return Some("No matching VAB header and body in this archive.".into());
    };
    let header = entry.sibling(header_index).to_vec();
    let body = entry.sibling(body_index).to_vec();
    let bank = match Bank::parse(&header, &body) {
        Ok(bank) => bank,
        Err(error) => return Some(format!("The VAB does not parse: {error}")),
    };
    let title = format!("{}-{header_index}", entry.archive.path.split().1.trim_end_matches(".EMI"));
    let mut message = None;

    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!(
                "VAB id {}  {} programs  {} tones  {} samples  volume {}  pan {}",
                bank.id, bank.program_count, bank.tone_count, bank.vag_count, bank.master_vol, bank.master_pan
            ))
            .monospace(),
        );
        if ui.button("Export SoundFont…").clicked() {
            let bytes = soundfont(&header, &body, &title).map_err(|error| error.to_string());
            message = save(&format!("{title}.sf2"), bytes);
        }
        if state.player.is_playing() && ui.button("Stop").clicked() {
            state.player.stop();
        }
    });
    ui.separator();

    ui.label(RichText::new("Samples").strong());
    egui::Grid::new("vags").striped(true).num_columns(6).show(ui, |ui| {
        for heading in ["#", "bytes", "frames", "loop", "", ""] {
            ui.label(RichText::new(heading).strong());
        }
        ui.end_row();

        for vag in bank.vags.iter().filter(|vag| !vag.is_empty()) {
            ui.label(vag.index.to_string());
            ui.label(vag.size.to_string());
            ui.label(vag.frames.to_string());
            ui.label(if vag.has_loop {
                format!("{}..{}", vag.loop_start_frame, vag.loop_end_frame)
            } else {
                "-".into()
            });

            let pcm = bank.decode_vag(&body, vag.index).map_err(|error| error.to_string());
            if ui.button("▶ Play").clicked() {
                message = match pcm.clone().and_then(|pcm| state.player.play(pcm)) {
                    Ok(()) => None,
                    Err(error) => Some(format!("Play: {error}")),
                };
            }
            if ui.button("WAVE…").clicked() {
                let bytes = pcm.and_then(|pcm| pcm_wav(&pcm).map_err(|error| error.to_string()));
                message = save(&format!("{title}-{}.wav", vag.index), bytes);
            }
            ui.end_row();
        }
    });

    ui.add_space(style::GAP);
    egui::CollapsingHeader::new(format!("Tones ({})", bank.tones.len())).show(ui, |ui| {
        egui::ScrollArea::vertical().max_height(style::LIST_HEIGHT).show(ui, |ui| {
            egui::Grid::new("tones").striped(true).show(ui, |ui| {
                for heading in ["program", "sample", "notes", "center", "volume", "pan", "mode"] {
                    ui.label(RichText::new(heading).strong());
                }
                ui.end_row();
                for tone in bank.usable_tones() {
                    ui.label(tone.program.to_string());
                    ui.label(tone.vag.to_string());
                    ui.label(format!("{}..{}", tone.min_note, tone.max_note));
                    ui.label(tone.center_note.to_string());
                    ui.label(tone.vol.to_string());
                    ui.label(tone.pan.to_string());
                    ui.label(tone.mode.to_string());
                    ui.end_row();
                }
            });
        });
    });
    message
}

fn sequences(ui: &mut egui::Ui, entry: &Entry<'_>) -> Option<String> {
    let payload = entry.payload().to_vec();
    let package = match Package::parse(&payload) {
        Ok(package) => package,
        Err(error) => return Some(format!("The sequence package does not parse: {error}")),
    };
    let title = format!("{}-{}", entry.archive.path.split().1.trim_end_matches(".EMI"), entry.index);
    let mut message = None;

    ui.label(RichText::new(format!("{} sequences", package.sequences.len())).strong());
    egui::Grid::new("sequences").striped(true).show(ui, |ui| {
        for (index, sequence) in package.sequences.iter().enumerate() {
            ui.label(format!("sequence {index}"));
            ui.label(if sequence.has_notes() { "notes" } else { "silent" });
            if ui.button("MIDI…").clicked() {
                let bytes = sequence_midi(&payload, index).map_err(|error| error.to_string());
                message = save(&format!("{title}-{index}.mid"), bytes);
            }
            ui.end_row();
        }
    });
    message
}
