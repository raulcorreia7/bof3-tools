//! A palette bank as a grid of 16-color rows, with a 15-bit color picker.
//!
//! A color word is `stp << 15 | blue << 10 | green << 5 | red`, five bits per channel. Word `0x0000`
//! draws transparent; the STP bit marks a semi-transparent texel.

use eframe::egui::{self, Color32, RichText, Vec2};
use rebof3_assets::decode::color_rgba;

use super::{Entry, style};

/// Colors in one row of a 4-bit palette, the grid's width.
const ROW_COLORS: usize = 16;
const CHANNEL_BITS: u32 = 5;
const CHANNEL_MAX: u16 = (1 << CHANNEL_BITS) - 1;
const GREEN_SHIFT: u32 = CHANNEL_BITS;
const BLUE_SHIFT: u32 = 2 * CHANNEL_BITS;
const STP_BIT: u16 = 1 << 15;

#[derive(Default)]
pub struct State {
    selected: usize,
}

struct Color {
    red: u16,
    green: u16,
    blue: u16,
    stp: bool,
}

impl Color {
    fn of(word: u16) -> Self {
        Self {
            red: word & CHANNEL_MAX,
            green: word >> GREEN_SHIFT & CHANNEL_MAX,
            blue: word >> BLUE_SHIFT & CHANNEL_MAX,
            stp: word & STP_BIT != 0,
        }
    }

    fn word(&self) -> u16 {
        let stp = if self.stp { STP_BIT } else { 0 };
        stp | self.blue << BLUE_SHIFT | self.green << GREEN_SHIFT | self.red
    }
}

pub fn show(ui: &mut egui::Ui, entry: &mut Entry<'_>, state: &mut State) -> Option<String> {
    let mut words: Vec<u16> = entry.payload().as_chunks::<2>().0.iter().map(|pair| u16::from_le_bytes(*pair)).collect();
    if words.is_empty() {
        return Some("The bank holds no colors.".into());
    }
    let original = entry.archive.original(entry.index);
    state.selected = state.selected.min(words.len() - 1);

    ui.label(
        RichText::new(format!("{} colors, {} rows of {ROW_COLORS}", words.len(), words.len().div_ceil(ROW_COLORS)))
            .monospace(),
    );
    ui.add_space(style::GAP_SMALL);

    let changed = picker(ui, &mut words[state.selected], state.selected);
    ui.separator();

    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        ui.spacing_mut().item_spacing = Vec2::splat(style::SWATCH_GAP);
        for (row, colors) in words.chunks(ROW_COLORS).enumerate() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{row:>4}")).monospace().weak());
                for (column, &word) in colors.iter().enumerate() {
                    let index = row * ROW_COLORS + column;
                    let [red, green, blue, alpha] = color_rgba(word);
                    let (rect, response) = ui.allocate_exact_size(Vec2::splat(style::SWATCH), egui::Sense::click());
                    ui.painter().rect_filled(
                        rect,
                        style::SQUARE,
                        Color32::from_rgba_unmultiplied(red, green, blue, alpha),
                    );

                    let edited = original.get(index * 2..index * 2 + 2) != Some(&word.to_le_bytes()[..]);
                    if index == state.selected {
                        ui.painter().rect_stroke(
                            rect,
                            style::SQUARE,
                            (style::OUTLINE_BOLD, style::SELECTION),
                            egui::StrokeKind::Inside,
                        );
                    } else if edited {
                        ui.painter().rect_stroke(
                            rect,
                            style::SQUARE,
                            (style::OUTLINE_THIN, style::EDITED),
                            egui::StrokeKind::Inside,
                        );
                    }
                    if response.on_hover_text(format!("{index}: {word:#06X}")).clicked() {
                        state.selected = index;
                    }
                }
            });
        }
    });

    if changed {
        let bytes: Vec<u8> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
        entry.set(bytes);
    }
    None
}

/// Channel sliders for one color. Returns true when the word changed.
fn picker(ui: &mut egui::Ui, word: &mut u16, index: usize) -> bool {
    let mut color = Color::of(*word);
    let before = *word;

    ui.horizontal(|ui| {
        let [red, green, blue, alpha] = color_rgba(*word);
        let (rect, _) =
            ui.allocate_exact_size(Vec2::splat(style::SWATCH * style::PREVIEW_SWATCHES), egui::Sense::hover());
        ui.painter().rect_filled(rect, style::ROUNDED, Color32::from_rgba_unmultiplied(red, green, blue, alpha));

        ui.vertical(|ui| {
            ui.label(RichText::new(format!("Color {index}: {:#06X}", *word)).monospace());
            ui.add(egui::Slider::new(&mut color.red, 0..=CHANNEL_MAX).text("red"));
            ui.add(egui::Slider::new(&mut color.green, 0..=CHANNEL_MAX).text("green"));
            ui.add(egui::Slider::new(&mut color.blue, 0..=CHANNEL_MAX).text("blue"));
            ui.checkbox(&mut color.stp, "semi-transparent (STP)");
        });
    });

    *word = color.word();
    *word != before
}
