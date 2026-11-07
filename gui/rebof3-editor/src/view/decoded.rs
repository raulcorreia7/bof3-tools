//! The structured reading of a format the decoders know, as a collapsible tree.

use eframe::egui::{self, RichText};
use rebof3_assets::decode::derive;
use rebof3_emi::classify::Format;
use serde_json::Value;

use super::Entry;

/// Array items shown before the rest folds into a count.
const ARRAY_PREVIEW: usize = 256;

pub fn decodes(format: Format) -> bool {
    matches!(
        format,
        Format::CueTable | Format::AreaActorDefsTable | Format::AreaScriptBank | Format::ClutBank | Format::Lz77Blob
    )
}

pub fn show(ui: &mut egui::Ui, entry: &Entry<'_>) -> Option<String> {
    let derived = match derive(entry.format(), entry.load_arg(), entry.payload()) {
        Ok(derived) => derived,
        Err(error) => return Some(format!("The decoder failed: {error}")),
    };

    egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
        for file in &derived {
            match file.role {
                "json" => match serde_json::from_slice::<Value>(&file.data) {
                    Ok(value) => tree(ui, "decoded", &value),
                    Err(error) => {
                        ui.label(format!("not JSON: {error}"));
                    }
                },
                "decoded" => {
                    ui.label(RichText::new(format!("Decompressed: {} bytes", file.data.len())).strong());
                }
                _ => {}
            }
        }
    });
    None
}

fn tree(ui: &mut egui::Ui, name: &str, value: &Value) {
    match value {
        Value::Object(fields) => {
            egui::CollapsingHeader::new(name).id_salt(ui.next_auto_id()).default_open(true).show(ui, |ui| {
                for (key, field) in fields {
                    tree(ui, key, field);
                }
            });
        }
        Value::Array(items) => {
            egui::CollapsingHeader::new(format!("{name} [{}]", items.len())).id_salt(ui.next_auto_id()).show(
                ui,
                |ui| {
                    for (index, item) in items.iter().take(ARRAY_PREVIEW).enumerate() {
                        tree(ui, &index.to_string(), item);
                    }
                    if items.len() > ARRAY_PREVIEW {
                        ui.label(format!("… {} more", items.len() - ARRAY_PREVIEW));
                    }
                },
            );
        }
        leaf => {
            ui.horizontal(|ui| {
                ui.label(RichText::new(name).strong());
                ui.label(RichText::new(leaf.to_string()).monospace());
            });
        }
    }
}
