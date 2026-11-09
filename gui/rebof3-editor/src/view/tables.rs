//! Record tables as spreadsheets: one row per record, one column per field. A reference column
//! (a drop's item, a level's ability, an encounter's enemy) shows the name it points at.

use gpui::{AnyElement, Context, Div, anchored, deferred, div, prelude::*, px};

use super::{Entry, Field, Menu, editing_menu, input};
use crate::app::{Editor, Menu as WindowMenu};
use crate::fonts;
use crate::schema::{self, Column, Field as Kind, Names, Table};
use crate::theme::{EDITED, MUTED, RAISED, color};
use crate::widgets::{
    GAP, GAP_SMALL, ROW_HEIGHT, button, checkbox, menu_item, menu_panel, muted, separator, strong, tab,
};

/// Width of a numeric cell, of a name cell, of a reference cell and of the row number, in pixels.
const NUMBER_WIDTH: f32 = 56.0;
const NAME_WIDTH: f32 = 120.0;
const REFERENCE_WIDTH: f32 = NUMBER_WIDTH + NAME_WIDTH;
const ROW_NUMBER_WIDTH: f32 = 40.0;
/// Width of the filter field, in pixels.
const FILTER_WIDTH: f32 = 160.0;

#[derive(Default)]
pub struct State {
    sheet: usize,
    pub filter: String,
}

/// The sheets of the shown entry.
fn sheets(entry: &Entry<'_>) -> Vec<Table> {
    schema::sheets_for(&entry.archive.path.0, entry.index, entry.format())
}

/// The names the reference columns of the shown entry show.
fn names(entry: &Entry<'_>) -> Names {
    Names::new(entry.elsewhere(schema::GAME, 0), Some(entry.payload()))
}

/// Whether the entry has a table layout.
pub fn has_tables(entry: &Entry<'_>) -> bool {
    !sheets(entry).is_empty()
}

pub fn show(editor: &Editor, entry: &Entry<'_>, cx: &mut Context<Editor>) -> AnyElement {
    let sheets = sheets(entry);
    if sheets.is_empty() {
        return muted("No table layout for this entry.").into_any_element();
    }
    let state = &editor.views.tables;
    let selected = state.sheet.min(sheets.len() - 1);

    let mut bar = div().flex().flex_none().flex_wrap().items_center().gap(px(GAP_SMALL));
    for (at, sheet) in sheets.iter().enumerate() {
        bar =
            bar.child(tab(("sheet", at), sheet.name, at == selected).on_click(cx.listener(move |editor, _, _, cx| {
                editor.views.tables.sheet = at;
                cx.notify();
            })));
    }
    bar = bar.child(separator(true)).child(input(
        editor,
        "table-filter",
        Field::TableFilter,
        &state.filter,
        "Filter by name",
        Some(FILTER_WIDTH),
        cx,
    ));

    let sheet = &sheets[selected];
    let payload = entry.payload();
    let original = entry.original();
    if payload.len() < sheet.end(payload) {
        return muted(format!("The payload ends before the {} table.", sheet.name)).into_any_element();
    }
    let names = names(entry);

    let mut header = div().flex().gap(px(GAP_SMALL)).child(strong("#").w(px(ROW_NUMBER_WIDTH)).flex_none());
    for column in &sheet.columns {
        header = header.child(strong(column.name).w(px(width(column))).flex_none().overflow_hidden());
    }

    let needle = state.filter.to_lowercase();
    let mut grid = div().id("table").flex().flex_col().flex_1().min_h_0().overflow_scroll().child(header);
    for row in 0..sheet.rows(payload) {
        if !needle.is_empty()
            && !row_name(payload, sheet, row).is_some_and(|name| name.to_lowercase().contains(&needle))
        {
            continue;
        }
        let range = sheet.range(row);
        let edited = payload[range.clone()] != original[range.clone()];
        let mut line = div()
            .id(("row", row))
            .flex()
            .flex_none()
            .items_center()
            .gap(px(GAP_SMALL))
            .min_h(px(ROW_HEIGHT))
            .when(row % 2 == 1, |line| line.bg(color(RAISED)))
            .child(
                div()
                    .w(px(ROW_NUMBER_WIDTH))
                    .flex_none()
                    .font_family(fonts::mono())
                    .text_color(if edited { color(EDITED) } else { color(MUTED) })
                    .child(row.to_string()),
            );
        for (at, column) in sheet.columns.iter().enumerate() {
            line = line.child(cell(editor, &names, &payload[range.clone()], row, at, column, cx));
        }
        grid = grid.child(line);
    }

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .gap(px(GAP))
        .child(bar)
        .child(separator(false))
        .child(grid)
        .into_any_element()
}

fn width(column: &Column) -> f32 {
    match column.field {
        Kind::Name(_) | Kind::Choice(_) => NAME_WIDTH,
        Kind::ItemAt(_) | Kind::ItemOf(_) | Kind::Ability | Kind::Enemy => REFERENCE_WIDTH,
        _ => NUMBER_WIDTH,
    }
}

fn row_name(payload: &[u8], sheet: &Table, row: usize) -> Option<String> {
    let column = sheet.columns.iter().find(|column| matches!(column.field, Kind::Name(_)))?;
    Some(schema::name_text(&payload[sheet.range(row)], column))
}

fn cell(
    editor: &Editor,
    names: &Names,
    record: &[u8],
    row: usize,
    at: usize,
    column: &Column,
    cx: &mut Context<Editor>,
) -> Div {
    let which = Field::Cell { row, column: at };
    let menu = Menu::Cell { row, column: at };
    let opened = editing_menu(editor, menu);
    let value = schema::read(record, column);
    let open_menu = cx.listener(move |editor, _, _, cx| {
        editor.menu = Some(WindowMenu::View(menu));
        cx.notify();
    });

    let content = match column.field {
        Kind::Name(_) => {
            let text = schema::name_text(record, column);
            input(editor, ("cell", at), which, &text, "", Some(NAME_WIDTH), cx).into_any_element()
        }
        Kind::Bits(names) => {
            let mut cell = div().flex().flex_col().child(
                button(("cell", at), format!("{value:08b}"), true).font_family(fonts::mono()).on_click(open_menu),
            );
            if opened {
                let mut panel = menu_panel("bits").on_mouse_down_out(close_menu(cx));
                for (bit, name) in names.iter().enumerate() {
                    let mask = 1 << bit;
                    panel = panel.child(checkbox(("bit", bit), name, value & mask != 0).on_click(cx.listener(
                        move |editor, _, _, cx| {
                            write_cell(editor, row, at, |value| value ^ mask);
                            cx.notify();
                        },
                    )));
                }
                cell = cell.child(deferred(anchored().snap_to_window().child(panel)));
            }
            cell.into_any_element()
        }
        Kind::Choice(choices) => {
            let selected = choices
                .iter()
                .find(|(code, _)| u32::from(*code) == value)
                .map_or_else(|| value.to_string(), |(_, name)| (*name).to_string());
            let mut cell = div()
                .flex()
                .flex_col()
                .child(button(("cell", at), format!("{selected} ▾"), true).w(px(NAME_WIDTH)).on_click(open_menu));
            if opened {
                let mut panel = menu_panel("choices").on_mouse_down_out(close_menu(cx));
                for &(code, name) in choices {
                    let mark = if u32::from(code) == value { "●" } else { "" };
                    panel = panel.child(menu_item(("choice", usize::from(code)), name, mark, true).on_click(
                        cx.listener(move |editor, _, _, cx| {
                            write_cell(editor, row, at, |_| u32::from(code));
                            editor.menu = None;
                            cx.notify();
                        }),
                    ));
                }
                cell = cell.child(deferred(anchored().snap_to_window().child(panel)));
            }
            cell.into_any_element()
        }
        Kind::U8 | Kind::U16 | Kind::U32 | Kind::Part { .. } => number(editor, which, at, value, cx).into_any_element(),
        Kind::ItemAt(_) | Kind::ItemOf(_) | Kind::Ability | Kind::Enemy => div()
            .flex()
            .items_center()
            .gap(px(GAP_SMALL))
            .child(number(editor, which, at, value, cx))
            .child(muted(names.of(record, column).unwrap_or_default().to_string()).w(px(NAME_WIDTH)).overflow_hidden())
            .into_any_element(),
    };
    div().w(px(width(column))).flex_none().child(content)
}

/// A number field that edits cell `which`.
fn number(editor: &Editor, which: Field, at: usize, value: u32, cx: &mut Context<Editor>) -> gpui::Stateful<Div> {
    input(editor, ("cell", at), which, &value.to_string(), "", Some(NUMBER_WIDTH), cx).font_family(fonts::mono())
}

fn close_menu(cx: &mut Context<Editor>) -> impl Fn(&gpui::MouseDownEvent, &mut gpui::Window, &mut gpui::App) + 'static {
    cx.listener(|editor, _, _, cx| {
        editor.menu = None;
        cx.notify();
    })
}

/// Rewrites the value of one cell through `change`.
fn write_cell(editor: &mut Editor, row: usize, at: usize, change: impl FnOnce(u32) -> u32) {
    let Some(entry) = editor.current() else { return };
    let Some(sheet) = sheets(&entry).into_iter().nth(editor.views.tables.sheet) else { return };
    let Some(column) = sheet.columns.get(at) else { return };
    let mut payload = entry.payload().to_vec();
    let Some(record) = payload.get_mut(sheet.range(row)) else { return };
    let value = change(schema::read(record, column)).min(schema::max(column));
    schema::write(record, column, value);
    editor.set_payload(payload);
}

/// Writes a cell's new text: a name, or a number up to the column's largest value.
pub fn cell_changed(editor: &mut Editor, row: usize, at: usize, text: &str) -> Option<String> {
    let entry = editor.current()?;
    let sheet = sheets(&entry).into_iter().nth(editor.views.tables.sheet)?;
    let column = sheet.columns.get(at)?;
    let mut payload = entry.payload().to_vec();
    let record = payload.get_mut(sheet.range(row))?;

    match column.field {
        Kind::Name(_) => match schema::name_bytes(text, column) {
            Ok(bytes) => record[column.offset..column.offset + bytes.len()].copy_from_slice(&bytes),
            Err(error) => return Some(format!("{}: {error}", column.name)),
        },
        _ => match text.trim().parse::<u32>() {
            Ok(value) if value <= schema::max(column) => schema::write(record, column, value),
            _ => return Some(format!("{}: a number from 0 to {}", column.name, schema::max(column))),
        },
    }
    editor.set_payload(payload);
    None
}
