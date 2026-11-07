//! What the archive and the classifier know about an entry.

use eframe::egui::{self, RichText};

use super::{Entry, style};

/// Space between the label and value columns, in points.
const INFO_COLUMN_GAP: f32 = 24.0;

pub fn show(ui: &mut egui::Ui, entry: &Entry<'_>) -> Option<String> {
    let toc = &entry.archive.entries()[entry.index];
    let format = entry.format();
    let original = entry.archive.original(entry.index).len();
    let current = entry.payload().len();

    egui::Grid::new("info").num_columns(2).striped(true).spacing([INFO_COLUMN_GAP, style::GAP_SMALL]).show(ui, |ui| {
        let mut row = |label: &str, value: String| {
            ui.label(RichText::new(label).strong());
            ui.label(RichText::new(value).monospace());
            ui.end_row();
        };

        row("Archive", entry.archive.path.0.clone());
        row("Entry", entry.index.to_string());
        row("Entry type", toc.file_type.to_string());
        row("Load argument", format!("{:#010X}", toc.ram_ptr));
        row("Disc offset", format!("{:#X}", toc.offset));
        row("Size on disc", format!("{original} bytes"));
        row("Size now", format!("{current} bytes"));
        row("Format", format.name().to_string());
        row("State", format.state().label().to_string());
        row("Spec", format.spec_state().to_string());
        row("Codec", format.codec_state().to_string());
    });

    ui.add_space(style::GAP_SECTION);
    ui.label(RichText::new("Evidence").strong());
    ui.label(RichText::new(format.evidence()).color(style::READ_ONLY));
    None
}
