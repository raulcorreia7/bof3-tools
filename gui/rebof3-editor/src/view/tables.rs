//! The fixed game tables as spreadsheets: one row per record, one column per field.

use eframe::egui::{self, RichText};

use super::{Entry, style};
use crate::schema::{self, Column, Field, Sheet};

#[derive(Default)]
pub struct State {
    sheet: usize,
    filter: String,
}

pub fn show(ui: &mut egui::Ui, entry: &mut Entry<'_>, state: &mut State) -> Option<String> {
    let sheets: Vec<Sheet> = schema::sheets()
        .into_iter()
        .filter(|sheet| sheet.archive == entry.archive.path.0 && sheet.entry == entry.index)
        .collect();
    if sheets.is_empty() {
        return Some("No table layout for this entry.".into());
    }
    state.sheet = state.sheet.min(sheets.len() - 1);

    ui.horizontal_wrapped(|ui| {
        for (at, sheet) in sheets.iter().enumerate() {
            ui.selectable_value(&mut state.sheet, at, sheet.table.name);
        }
        ui.separator();
        ui.add(
            egui::TextEdit::singleline(&mut state.filter)
                .hint_text("Filter by name")
                .desired_width(style::FILTER_WIDTH),
        );
    });
    ui.separator();

    let sheet = &sheets[state.sheet];
    let mut payload = entry.payload().to_vec();
    let original = entry.archive.original(entry.index).to_vec();
    let end = sheet.start() + sheet.table.count * sheet.table.record;
    if payload.len() < end {
        return Some(format!("The payload ends before the {} table.", sheet.table.name));
    }

    let mut message = None;
    let filter = state.filter.to_lowercase();
    let rows: Vec<usize> = (0..sheet.table.count)
        .filter(|&row| {
            filter.is_empty()
                || row_name(&payload, sheet, row).is_some_and(|name| name.to_lowercase().contains(&filter))
        })
        .collect();

    egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
        egui::Grid::new(sheet.table.name).striped(true).num_columns(sheet.columns.len() + 1).show(ui, |ui| {
            ui.label(RichText::new("#").strong());
            for column in &sheet.columns {
                ui.label(RichText::new(column.name).strong());
            }
            ui.end_row();

            for &row in &rows {
                let start = sheet.start() + row * sheet.table.record;
                let range = start..start + sheet.table.record;
                let edited = payload[range.clone()] != original[range.clone()];
                let number = RichText::new(row.to_string()).monospace();
                ui.label(if edited { number.color(style::EDITED) } else { number.weak() });

                for column in &sheet.columns {
                    cell(ui, &mut payload[range.clone()], column, &mut message);
                }
                ui.end_row();
            }
        });
    });

    if payload.as_slice() != entry.payload() {
        entry.set(payload);
    }
    message
}

fn row_name(payload: &[u8], sheet: &Sheet, row: usize) -> Option<String> {
    let column = sheet.columns.iter().find(|column| matches!(column.field, Field::Name))?;
    let start = sheet.start() + row * sheet.table.record;
    Some(schema::name_text(&payload[start..start + sheet.table.record], column.offset))
}

fn cell(ui: &mut egui::Ui, record: &mut [u8], column: &Column, message: &mut Option<String>) {
    match column.field {
        Field::Name => {
            let mut text = schema::name_text(record, column.offset);
            let response = ui.add(egui::TextEdit::singleline(&mut text).desired_width(style::NAME_WIDTH));
            if response.changed() {
                match schema::name_bytes(&text) {
                    Ok(bytes) => record[column.offset..column.offset + bytes.len()].copy_from_slice(&bytes),
                    Err(error) => *message = Some(format!("{}: {error}", column.name)),
                }
            }
        }
        Field::Bits(names) => {
            let mut value = schema::read(record, column);
            ui.menu_button(RichText::new(format!("{value:08b}")).monospace(), |ui| {
                for (bit, name) in names.iter().enumerate() {
                    let mask = 1 << bit;
                    let mut set = value & mask != 0;
                    if ui.checkbox(&mut set, *name).changed() {
                        value ^= mask;
                    }
                }
            });
            schema::write(record, column, value);
        }
        Field::Choice(choices) => {
            let mut value = schema::read(record, column);
            let selected = choices
                .iter()
                .find(|(code, _)| u32::from(*code) == value)
                .map_or_else(|| format!("{value}"), |(_, name)| (*name).into());
            egui::ComboBox::from_id_salt(ui.next_auto_id()).selected_text(selected).width(style::NAME_WIDTH).show_ui(
                ui,
                |ui| {
                    for (code, name) in choices {
                        ui.selectable_value(&mut value, u32::from(*code), *name);
                    }
                },
            );
            schema::write(record, column, value);
        }
        Field::U8 | Field::U16 | Field::Nibble(_) => {
            let mut value = schema::read(record, column);
            ui.add_sized(
                [style::NUMBER_WIDTH, ui.spacing().interact_size.y],
                egui::DragValue::new(&mut value).range(0..=schema::max(column)),
            );
            schema::write(record, column, value);
        }
    }
}
