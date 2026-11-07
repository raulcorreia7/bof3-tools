//! The strings of an entry: a message bank's records, or the strings a scan finds in any other payload.
//!
//! A string is rewritten in place. New text must encode to no more bytes than the old string took; the
//! rest of its span is filled with terminators, so no offset table or pointer moves.

use eframe::egui::{self, RichText};
use rebof3_emi::text::codec::{self, Encoding};
use rebof3_emi::text::discover::{self, Found, Kind};

use super::{Entry, style};

/// The byte that ends a string and pads a shortened one.
const TERMINATOR: u8 = 0;

#[derive(Default)]
pub struct State {
    filter: String,
}

fn found(payload: &[u8], load_arg: u32) -> Vec<Found> {
    discover::bank_records(payload).unwrap_or_else(|| discover::strings(payload, Some(load_arg)))
}

pub fn has_text(payload: &[u8], load_arg: u32) -> bool {
    !found(payload, load_arg).is_empty()
}

fn encoding(kind: Kind) -> Encoding {
    match kind {
        Kind::Field { .. } => Encoding::Name,
        Kind::Message | Kind::Record => Encoding::Message,
    }
}

pub fn show(ui: &mut egui::Ui, entry: &mut Entry<'_>, state: &mut State) -> Option<String> {
    let mut payload = entry.payload().to_vec();
    let original = entry.archive.original(entry.index).to_vec();
    let strings = found(&payload, entry.load_arg());

    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("{} strings", strings.len())).strong());
        ui.add(egui::TextEdit::singleline(&mut state.filter).hint_text("Filter").desired_width(200.0));
        ui.label(RichText::new("New text must fit the old string's bytes.").weak());
    });
    ui.separator();

    let mut message = None;
    let filter = state.filter.to_lowercase();
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        for string in &strings {
            let encoding = encoding(string.kind);
            let span = string.start..string.end;
            let text = codec::decode(encoding, &payload[span.clone()]);
            if !filter.is_empty() && !text.to_lowercase().contains(&filter) {
                continue;
            }

            ui.horizontal(|ui| {
                let edited = payload[span.clone()] != original.get(span.clone()).unwrap_or_default()[..];
                let offset = RichText::new(format!("{:06X}", string.start)).monospace();
                ui.label(if edited { offset.color(style::EDITED) } else { offset.weak() });

                let mut draft = text.clone();
                let response =
                    ui.add(egui::TextEdit::multiline(&mut draft).desired_rows(1).desired_width(f32::INFINITY));
                if !response.changed() {
                    return;
                }
                match codec::encode(encoding, &draft) {
                    Ok(bytes) if bytes.len() <= span.len() => {
                        payload[span.clone()].fill(TERMINATOR);
                        payload[string.start..string.start + bytes.len()].copy_from_slice(&bytes);
                    }
                    Ok(bytes) => {
                        message = Some(format!("Too long: {} bytes, the string has {}", bytes.len(), span.len()))
                    }
                    Err(error) => message = Some(format!("Cannot encode: {error}")),
                }
            });
        }
    });

    if payload.as_slice() != entry.payload() {
        entry.set(payload);
    }
    message
}
