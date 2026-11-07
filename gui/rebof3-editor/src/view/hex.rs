//! A hex editor with a value inspector: the bytes at the cursor read as every integer width.

use eframe::egui::{self, Color32, RichText};

use super::{Entry, style};

/// Bytes shown on one row.
const ROW_BYTES: usize = 16;
/// Hex digits in a byte.
const BYTE_DIGITS: usize = 2;
/// Space between two bytes of a row, in points.
const BYTE_GAP: f32 = 4.0;
/// The value of 1.0 in 16.16 fixed point.
const FIXED_ONE: f64 = 65_536.0;
/// Width of an inspector field, in points.
const FIELD_WIDTH: f32 = 120.0;
/// Width of the go-to field, in points.
const GOTO_WIDTH: f32 = 80.0;

#[derive(Default)]
pub struct State {
    cursor: usize,
    /// The first hex digit typed into the cursor byte, waiting for the second.
    pending: Option<u8>,
    goto: String,
}

pub fn show(ui: &mut egui::Ui, entry: &mut Entry<'_>, state: &mut State) -> Option<String> {
    let mut payload = entry.payload().to_vec();
    if payload.is_empty() {
        ui.label("The entry is empty.");
        return None;
    }
    state.cursor = state.cursor.min(payload.len() - 1);
    let original = entry.archive.original(entry.index).to_vec();
    let mut message = None;

    ui.horizontal(|ui| {
        ui.label("Go to offset");
        let response = ui.add(egui::TextEdit::singleline(&mut state.goto).desired_width(GOTO_WIDTH).hint_text("0x0"));
        if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
            match parse_offset(&state.goto) {
                Some(offset) if offset < payload.len() => state.cursor = offset,
                _ => message = Some(format!("Offset must be below {:#X}", payload.len())),
            }
        }
        ui.separator();
        ui.label(RichText::new("Type hex digits to overwrite the byte at the cursor; arrows move.").weak());
    });

    ui.columns(2, |columns| {
        inspector(&mut columns[1], &mut payload, state.cursor, &mut message);
        grid(&mut columns[0], &payload, &original, state);
    });

    if typed(ui, &mut payload, state) || payload.as_slice() != entry.payload() {
        entry.set(payload);
    }
    message
}

fn parse_offset(text: &str) -> Option<usize> {
    let text = text.trim();
    match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => usize::from_str_radix(hex, 16).ok(),
        None => text.parse().ok(),
    }
}

/// Applies keyboard input to the cursor byte. Returns true when a byte changed.
fn typed(ui: &egui::Ui, payload: &mut [u8], state: &mut State) -> bool {
    if ui.ctx().egui_wants_keyboard_input() {
        return false;
    }
    let last = payload.len() - 1;
    let mut changed = false;
    ui.input(|input| {
        for event in &input.events {
            match event {
                egui::Event::Key { key, pressed: true, .. } => {
                    let step = match key {
                        egui::Key::ArrowRight => 1isize,
                        egui::Key::ArrowLeft => -1,
                        egui::Key::ArrowDown => ROW_BYTES as isize,
                        egui::Key::ArrowUp => -(ROW_BYTES as isize),
                        _ => 0,
                    };
                    if step != 0 {
                        state.cursor = state.cursor.saturating_add_signed(step).min(last);
                        state.pending = None;
                    }
                }
                egui::Event::Text(text) => {
                    for digit in text.chars().filter_map(|c| c.to_digit(16)) {
                        let digit = digit as u8;
                        match state.pending.take() {
                            None => state.pending = Some(digit),
                            Some(high) => {
                                payload[state.cursor] = high << 4 | digit;
                                state.cursor = (state.cursor + 1).min(last);
                                changed = true;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    });
    changed
}

fn grid(ui: &mut egui::Ui, payload: &[u8], original: &[u8], state: &mut State) {
    let rows = payload.len().div_ceil(ROW_BYTES);
    let row_height = ui.text_style_height(&egui::TextStyle::Monospace);

    egui::ScrollArea::vertical().id_salt("hex").auto_shrink(false).show_rows(ui, row_height, rows, |ui, range| {
        for row in range {
            let start = row * ROW_BYTES;
            let end = (start + ROW_BYTES).min(payload.len());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = BYTE_GAP;
                ui.label(RichText::new(format!("{start:08X}")).monospace().weak());

                for (at, &byte) in payload.iter().enumerate().take(end).skip(start) {
                    let edited = original.get(at) != Some(&byte);
                    let text = if at == state.cursor && state.pending.is_some() {
                        format!("{:X}_", state.pending.unwrap_or_default())
                    } else {
                        format!("{byte:0width$X}", width = BYTE_DIGITS)
                    };
                    let mut text = RichText::new(text).monospace();
                    if edited {
                        text = text.color(style::EDITED);
                    }
                    if at == state.cursor {
                        text = text.background_color(style::SELECTION).color(Color32::BLACK);
                    }
                    if ui.add(egui::Label::new(text).sense(egui::Sense::click())).clicked() {
                        state.cursor = at;
                        state.pending = None;
                    }
                }

                let ascii: String = payload[start..end]
                    .iter()
                    .map(|&byte| if byte.is_ascii_graphic() { byte as char } else { '.' })
                    .collect();
                ui.label(RichText::new(ascii).monospace().weak());
            });
        }
    });
}

/// The integer widths at the cursor, each editable.
fn inspector(ui: &mut egui::Ui, payload: &mut [u8], cursor: usize, message: &mut Option<String>) {
    ui.label(RichText::new(format!("Cursor {cursor:#X}")).strong());
    egui::Grid::new("inspector").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
        width_row(ui, payload, cursor, 1, false, "u8", message);
        width_row(ui, payload, cursor, 1, true, "s8", message);
        width_row(ui, payload, cursor, 2, false, "u16", message);
        width_row(ui, payload, cursor, 2, true, "s16", message);
        width_row(ui, payload, cursor, 4, false, "u32", message);
        width_row(ui, payload, cursor, 4, true, "s32", message);

        if let Some(bytes) = payload.get(cursor..cursor + 4) {
            let word = u32::from_le_bytes(bytes.try_into().unwrap_or_default());
            ui.label("16.16 fixed");
            ui.label(RichText::new(format!("{:.4}", f64::from(word as i32) / FIXED_ONE)).monospace());
            ui.end_row();
            ui.label("pointer");
            ui.label(RichText::new(format!("{word:#010X}")).monospace());
            ui.end_row();
        }
    });
}

fn width_row(
    ui: &mut egui::Ui,
    payload: &mut [u8],
    cursor: usize,
    width: usize,
    signed: bool,
    label: &str,
    message: &mut Option<String>,
) {
    let Some(bytes) = payload.get(cursor..cursor + width) else { return };
    let mut raw = [0u8; 8];
    raw[..width].copy_from_slice(bytes);
    let unsigned = u64::from_le_bytes(raw);
    let bits = width as u32 * 8;
    let value =
        if signed { ((unsigned << (64 - bits)) as i64 >> (64 - bits)).to_string() } else { unsigned.to_string() };

    ui.label(label);
    let mut text = value.clone();
    let response =
        ui.add(egui::TextEdit::singleline(&mut text).desired_width(FIELD_WIDTH).font(egui::TextStyle::Monospace));
    ui.end_row();
    if !response.changed() || text == value {
        return;
    }

    let parsed =
        if signed { text.trim().parse::<i64>().ok().map(|v| v as u64) } else { text.trim().parse::<u64>().ok() };
    let limit = if bits == 64 { u64::MAX } else { (1u64 << bits) - 1 };
    match parsed {
        Some(number) => {
            let stored = (number & limit).to_le_bytes();
            payload[cursor..cursor + width].copy_from_slice(&stored[..width]);
        }
        None => *message = Some(format!("{label}: not a number")),
    }
}
