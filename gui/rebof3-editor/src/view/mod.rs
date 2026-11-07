//! The views of one entry. Each view reads the entry's current payload and writes a whole new
//! payload back through [`Project::set`], so undo, save and export see every view's edits the same
//! way.

mod decoded;
mod hex;
mod info;
mod palette;
mod sound;
mod tables;
mod text;
mod texture;

use eframe::egui;
use rebof3_emi::classify::Format;

use crate::project::{Loaded, Project};
use crate::schema;

/// Colors and sizes shared by the views.
pub mod style {
    use eframe::egui::Color32;

    /// An edited entry, byte or cell.
    pub const EDITED: Color32 = Color32::from_rgb(230, 170, 60);
    /// A value the view cannot write back.
    pub const READ_ONLY: Color32 = Color32::from_rgb(150, 150, 160);
    /// The outline of the selected texel or color.
    pub const SELECTION: Color32 = Color32::from_rgb(255, 230, 80);

    /// The side of one swatch in a palette grid, in points.
    pub const SWATCH: f32 = 18.0;
    /// Width of a numeric cell in a table, in points.
    pub const NUMBER_WIDTH: f32 = 56.0;
    /// Width of a name cell in a table, in points.
    pub const NAME_WIDTH: f32 = 120.0;
    /// Width of a filter field, in points.
    pub const FILTER_WIDTH: f32 = 160.0;
    /// Width of a palette chooser, in points.
    pub const CHOOSER_WIDTH: f32 = 320.0;
    /// Height of a list that scrolls inside a view, in points.
    pub const LIST_HEIGHT: f32 = 320.0;

    /// Small, normal and section spacing, in points.
    pub const GAP_SMALL: f32 = 4.0;
    pub const GAP: f32 = 8.0;
    pub const GAP_SECTION: f32 = 12.0;
    /// Space between two swatches of a palette row, in points.
    pub const SWATCH_GAP: f32 = 1.0;
    /// The preview swatch of the color picker, in swatches.
    pub const PREVIEW_SWATCHES: f32 = 3.0;

    /// Outline widths, in points.
    pub const OUTLINE_THIN: f32 = 1.0;
    pub const OUTLINE_BOLD: f32 = 2.0;
    /// Corner radius of a swatch (square) and of the picker preview, in points.
    pub const SQUARE: f32 = 0.0;
    pub const ROUNDED: f32 = 4.0;
}

/// One view of an entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tab {
    Info,
    Hex,
    Texture,
    Palette,
    Tables,
    Text,
    Sound,
    Decoded,
}

impl Tab {
    fn label(self) -> &'static str {
        match self {
            Tab::Info => "Info",
            Tab::Hex => "Hex",
            Tab::Texture => "Texture",
            Tab::Palette => "Palette",
            Tab::Tables => "Tables",
            Tab::Text => "Text",
            Tab::Sound => "Sound",
            Tab::Decoded => "Decoded",
        }
    }
}

/// The state each view keeps between frames.
#[derive(Default)]
pub struct Views {
    hex: hex::State,
    texture: texture::State,
    palette: palette::State,
    tables: tables::State,
    text: text::State,
    sound: sound::State,
}

/// What a view works on: the project, the archive and the entry index.
pub struct Entry<'a> {
    pub project: &'a mut Project,
    pub archive: &'a Loaded,
    pub index: usize,
}

impl Entry<'_> {
    pub fn format(&self) -> Format {
        self.archive.formats[self.index]
    }

    pub fn load_arg(&self) -> u32 {
        self.archive.entries()[self.index].ram_ptr
    }

    pub fn payload(&self) -> &[u8] {
        self.project.payload(self.archive, self.index)
    }

    pub fn set(&mut self, bytes: Vec<u8>) {
        self.project.set(self.archive, self.index, bytes);
    }

    /// The payload of another entry of the same archive.
    pub fn sibling(&self, index: usize) -> &[u8] {
        self.project.payload(self.archive, index)
    }
}

/// The views that apply to an entry, Info and Hex first.
pub fn tabs(archive: &Loaded, index: usize, payload: &[u8]) -> Vec<Tab> {
    let format = archive.formats[index];
    let mut tabs = vec![Tab::Info, Hex];
    match format {
        Format::VramUpload => tabs.push(Tab::Texture),
        Format::ClutBank => tabs.push(Tab::Palette),
        Format::VabHeader | Format::VabBody | Format::SepPackage => tabs.push(Tab::Sound),
        _ => {}
    }
    if schema::sheets().iter().any(|sheet| sheet.archive == archive.path.0 && sheet.entry == index) {
        tabs.push(Tab::Tables);
    }
    if text::has_text(payload, archive.entries()[index].ram_ptr) {
        tabs.push(Tab::Text);
    }
    if decoded::decodes(format) {
        tabs.push(Tab::Decoded);
    }
    tabs
}

use Tab::Hex;

/// Draws the tab bar and the selected view. Returns a status message when the view has one.
pub fn show(
    ui: &mut egui::Ui,
    project: &mut Project,
    archive: &Loaded,
    index: usize,
    tab: &mut Tab,
    views: &mut Views,
) -> Option<String> {
    let available = tabs(archive, index, project.payload(archive, index));
    if !available.contains(tab) {
        *tab = Tab::Info;
    }

    ui.horizontal(|ui| {
        for candidate in &available {
            ui.selectable_value(tab, *candidate, candidate.label());
        }
    });
    ui.separator();

    let mut entry = Entry { project, archive, index };
    match tab {
        Tab::Info => info::show(ui, &entry),
        Tab::Hex => hex::show(ui, &mut entry, &mut views.hex),
        Tab::Texture => texture::show(ui, &mut entry, &mut views.texture),
        Tab::Palette => palette::show(ui, &mut entry, &mut views.palette),
        Tab::Tables => tables::show(ui, &mut entry, &mut views.tables),
        Tab::Text => text::show(ui, &mut entry, &mut views.text),
        Tab::Sound => sound::show(ui, &mut entry, &mut views.sound),
        Tab::Decoded => decoded::show(ui, &entry),
    }
}
