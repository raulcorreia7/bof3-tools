//! A script table as the programs its records hold: one animation stream per record, listed one op
//! per line.
//!
//! `Actor_BeginScript` (`exe/slus_004_22@0x8014D6DC`) points an actor at a record of the table and
//! `Actor_SeekScript` (`exe/slus_004_22@0x8014D910`) plays it: each step is a `show` (a frame and the
//! ticks it stays) or a `jump` (the step to continue at), [`rebof3_formats::script::program::OPCODES`].
//! Editing an operand rewrites its byte and keeps every other byte. A step is added or removed at the
//! end of a stream only: game code names stream positions (the start position `Actor_BeginScript`
//! takes, `script_pc` tests in the area and party code), and a step moved mid-stream would change
//! what they name. No edit may grow the table past its stored size.

use gpui::{AnyElement, Context, Div, Stateful, div, prelude::*, px};
use rebof3_formats::script::program::{CellAt, Op, Program, Record, Section, StepAt, Stream};
use rebof3_formats::script::text;
use rebof3_formats::sprite::{Cell, JUMP, duration_ms, ticks_of};

use super::{Entry, Field, input};
use crate::app::Editor;
use crate::fonts;
use crate::theme::{EDITED, MUTED, RAISED, color};
use crate::widgets::{GAP, GAP_SMALL, ROW_HEIGHT, button, mono, muted, selectable, separator, strong, tab, tip};

/// Width of the record list, of a number field, of a step's label and index columns, in pixels.
const LIST_WIDTH: f32 = 300.0;
const NUMBER_WIDTH: f32 = 52.0;
const LABEL_WIDTH: f32 = 40.0;
const INDEX_WIDTH: f32 = 32.0;
const MNEMONIC_WIDTH: f32 = 48.0;
/// Bytes on one line of a raw record's dump.
const DUMP_BYTES: usize = 16;
/// The ticks a step the view appends stays, when the stream shows no frame to copy them from.
const APPENDED_TICKS: u8 = 1;

/// The shown section, record and the step a jump label picked.
#[derive(Default)]
pub struct State {
    section: usize,
    record: usize,
    focus: Option<usize>,
}

/// One operand field of a step.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StepPart {
    Ticks,
    Frame,
    Target,
    Code,
}

/// One field of a cell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CellPart {
    Size,
    X,
    Y,
    U,
    V,
}

pub fn show(editor: &Editor, entry: &Entry<'_>, cx: &mut Context<Editor>) -> AnyElement {
    let program = match Program::decode(entry.payload()) {
        Ok(program) => program,
        Err(error) => return muted(format!("Not a script table: {error}")).into_any_element(),
    };
    let original = Program::decode(entry.original()).ok();
    let state = &editor.views.script;
    let section_index = state.section.min(program.sections.len() - 1);
    let section = &program.sections[section_index];
    let record_index = state.record.min(section.records.len().saturating_sub(1));

    let ops: usize = program.sections.iter().flat_map(|section| &section.records).filter_map(Record::stream).map(|stream| stream.animation.steps.len()).sum();
    let records: usize = program.sections.iter().map(|section| section.records.len()).sum();
    let summary = format!("{} sections, {records} records, {ops} ops. Actor_BeginScript points an actor at a record; Actor_SeekScript plays it.", program.sections.len());

    let mut bar = div().flex().flex_wrap().items_center().gap(px(GAP_SMALL)).child("Section");
    for number in 0..program.sections.len() {
        bar = bar.child(tab(("script-section", number), number.to_string(), number == section_index).on_click(cx.listener(move |editor, _, _, cx| {
            editor.views.script = State { section: number, record: 0, focus: None };
            cx.notify();
        })));
    }
    bar = bar
        .child(separator(true))
        .child(button("script-export", "Export text", true).on_click(cx.listener(|editor, _, _, cx| export(editor, cx))))
        .child(button("script-import", "Import text", true).on_click(cx.listener(|editor, _, _, cx| import(editor, cx))));

    let original_section = original.as_ref().and_then(|program| program.sections.get(section_index));
    let body = div().flex().flex_1().min_h_0().gap(px(GAP)).child(record_list(section, original_section, record_index, cx)).child(separator(true)).child(listing(
        editor,
        section_index,
        section,
        record_index,
        cx,
    ));
    div().flex().flex_col().flex_1().min_h_0().gap(px(GAP)).child(muted(summary)).child(bar).child(separator(false)).child(body).into_any_element()
}

/// Every record of the section: the table entries that name it, and its shape.
fn record_list(section: &Section, original: Option<&Section>, shown: usize, cx: &mut Context<Editor>) -> Stateful<Div> {
    let mut list = div().id("script-records").flex().flex_col().w(px(LIST_WIDTH)).flex_none().overflow_y_scroll();
    for (index, record) in section.records.iter().enumerate() {
        let entries: Vec<String> = section.slots.iter().enumerate().filter(|(_, slot)| usize::from(**slot) == index).map(|(entry, _)| entry.to_string()).collect();
        let shape = match record {
            Record::Stream(stream) => {
                let looping = stream.animation.loop_to().map_or(String::new(), |to| format!(", loops to {}", text::label(to)));
                format!("{} ops, {} frames{looping}", stream.animation.steps.len(), stream.animation.frames.len())
            }
            Record::Raw { bytes, .. } => format!("raw, {} bytes", bytes.len()),
        };
        let edited = original.and_then(|original| original.records.get(index)) != Some(record);
        let line = format!("{index:>3}  entries {}  {shape}", entries.join(" "));
        let row = selectable(("script-record", index), index == shown).min_h(px(ROW_HEIGHT)).child(mono(line).when(edited, |line| line.text_color(color(EDITED)))).on_click(cx.listener(
            move |editor, _, _, cx| {
                editor.views.script.record = index;
                editor.views.script.focus = None;
                cx.notify();
            },
        ));
        list = list.child(row);
    }
    list
}

/// The shown record: its ops, its frames and its padding, or its raw bytes.
fn listing(editor: &Editor, section: usize, shown: &Section, record: usize, cx: &mut Context<Editor>) -> Stateful<Div> {
    let column = div().id("script-listing").flex().flex_col().flex_1().min_w_0().gap(px(GAP)).overflow_y_scroll();
    let Some(found) = shown.records.get(record) else { return column.child(muted("The section has no records.")) };
    let stream = match found {
        Record::Stream(stream) => stream,
        Record::Raw { bytes, reason } => {
            let mut dump = div().flex().flex_col();
            for (line, chunk) in bytes.chunks(DUMP_BYTES).enumerate() {
                let hex: Vec<String> = chunk.iter().map(|byte| format!("{byte:02x}")).collect();
                dump = dump.child(mono(format!("{:04x}  {}", line * DUMP_BYTES, hex.join(" "))));
            }
            return column.child(muted(format!("Not an animation stream: {reason}"))).child(dump);
        }
    };

    let mut ops = div().flex().flex_col().gap(px(GAP_SMALL / 2.0));
    let targets = stream.targets();
    let focus = editor.views.script.focus;
    for (index, op) in stream.ops().enumerate() {
        let at = StepAt { section, record, step: index };
        let label = if targets.contains(&index) { format!("{}:", text::label(index)) } else { String::new() };
        let mut row = div()
            .id(("script-op", index))
            .flex()
            .flex_none()
            .items_center()
            .gap(px(GAP_SMALL))
            .min_h(px(ROW_HEIGHT))
            .when(focus == Some(index), |row| row.bg(color(RAISED)))
            .child(mono(label).w(px(LABEL_WIDTH)).flex_none())
            .child(mono(index.to_string()).w(px(INDEX_WIDTH)).flex_none().text_color(color(MUTED)))
            .child(strong(op.opcode().mnemonic).id(("script-mnemonic", index)).w(px(MNEMONIC_WIDTH)).flex_none().tooltip(tip(format!("{}: {}", op.opcode().handler, op.opcode().does))));
        row = match op {
            Op::Show { ticks, frame } => {
                let shown_frame = match stream.animation.frames.get(usize::from(frame)) {
                    Some(cells) => format!("{} cells", cells.cells.len()),
                    None => "past the frame table: the game reads a word of the frames".into(),
                };
                row.child("frame")
                    .child(number(editor, at, StepPart::Frame, frame, cx))
                    .child(muted(shown_frame))
                    .child("ticks")
                    .child(number(editor, at, StepPart::Ticks, ticks, cx))
                    .child(muted(format!("{} ticks, {} ms", ticks_of(ticks), duration_ms(ticks))))
            }
            Op::Jump { code, target } => {
                let target_index = usize::from(target);
                row.child("to")
                    .child(tab(("script-target", index), text::label(target_index), false).on_click(cx.listener(move |editor, _, _, cx| {
                        editor.views.script.focus = Some(target_index);
                        cx.notify();
                    })))
                    .child(number(editor, at, StepPart::Target, target, cx))
                    .child("code")
                    .child(number(editor, at, StepPart::Code, code, cx))
            }
        };
        ops = ops.child(row);
    }

    let end = StepAt { section, record, step: stream.animation.steps.len() };
    let removable = end.step > 1;
    let edits = div()
        .flex()
        .items_center()
        .gap(px(GAP_SMALL))
        .child(button("script-append", "Append a show", true).on_click(cx.listener(move |editor, _, _, cx| {
            let ticks = appended_ticks(editor, end);
            report(editor, |program| program.insert_op(end, Op::Show { ticks, frame: 0 }));
            cx.notify();
        })))
        .child(button("script-remove", "Remove the last op", removable).when(removable, |remove| {
            remove.on_click(cx.listener(move |editor, _, _, cx| {
                let last = StepAt { step: end.step - 1, ..end };
                report(editor, |program| program.remove_op(last));
                cx.notify();
            }))
        }))
        .child(muted("Steps change at the end only: game code names stream positions."));

    column.child(strong(format!("Record {record}: {} ops", stream.animation.steps.len()))).child(ops).child(edits).child(separator(false)).child(frames(editor, section, record, stream, cx))
}

/// The frames of a stream with an editable row per cell.
fn frames(editor: &Editor, section: usize, record: usize, stream: &Stream, cx: &mut Context<Editor>) -> Div {
    let mut list = div().flex().flex_col().gap(px(GAP_SMALL / 2.0));
    for (frame, cells) in stream.animation.frames.iter().enumerate() {
        list = list.child(strong(format!("Frame {frame}: {} cells", cells.cells.len())));
        for (index, cell) in cells.cells.iter().enumerate() {
            let at = CellAt { section, record, frame, cell: index };
            let mut row = div().flex().items_center().gap(px(GAP_SMALL)).pl(px(LABEL_WIDTH));
            for (name, part, value) in [
                ("size", CellPart::Size, i32::from(cell.size)),
                ("x", CellPart::X, i32::from(cell.x)),
                ("y", CellPart::Y, i32::from(cell.y)),
                ("u", CellPart::U, i32::from(cell.u)),
                ("v", CellPart::V, i32::from(cell.v)),
            ] {
                let which = Field::ScriptCell { at, part };
                row = row.child(name).child(input(editor, ("script-cell", cell_id(at, part)), which, &value.to_string(), "", Some(NUMBER_WIDTH), cx).font_family(fonts::mono()));
            }
            let flipped = if cell.flipped() { ", flipped" } else { "" };
            list = list.child(row.child(muted(format!("shape {}{flipped}", cell.shape()))));
        }
    }
    if !stream.padding.is_empty() {
        list = list.child(muted(format!("{} bytes of padding before the next record", stream.padding.len())));
    }
    list
}

/// The element id of one cell field: unique over the frames and cells of a stream.
fn cell_id(at: CellAt, part: CellPart) -> usize {
    const PARTS: usize = 8;
    const CELLS: usize = 256;
    (at.frame * CELLS + at.cell) * PARTS + part as usize
}

/// A number field that edits one byte of step `at`.
fn number(editor: &Editor, at: StepAt, part: StepPart, value: u8, cx: &mut Context<Editor>) -> Stateful<Div> {
    let which = Field::ScriptStep { at, part };
    input(editor, ("script-step", at.step * 4 + part as usize), which, &value.to_string(), "", Some(NUMBER_WIDTH), cx).font_family(fonts::mono())
}

/// The ticks an appended step takes: the last show's, or [`APPENDED_TICKS`].
fn appended_ticks(editor: &Editor, end: StepAt) -> u8 {
    let Some(entry) = editor.current() else { return APPENDED_TICKS };
    let Ok(program) = Program::decode(entry.payload()) else { return APPENDED_TICKS };
    program.stream(end.section, end.record).and_then(|stream| stream.ops().filter_map(|op| if let Op::Show { ticks, .. } = op { Some(ticks) } else { None }).last()).unwrap_or(APPENDED_TICKS)
}

/// The payload after `change`, refused when the table would grow past `limit`, its stored size: what
/// follows the table in memory is not known to be free.
pub fn rewritten(payload: &[u8], limit: usize, change: impl FnOnce(&mut Program) -> rebof3_formats::Result<()>) -> Result<Vec<u8>, String> {
    let mut program = Program::decode(payload).map_err(|error| error.to_string())?;
    change(&mut program).map_err(|error| error.to_string())?;
    let bytes = program.encode().map_err(|error| error.to_string())?;
    if bytes.len() > limit {
        return Err(format!("the table would grow to {} bytes past its stored {limit}: what follows it in memory is not known to be free", bytes.len()));
    }
    Ok(bytes)
}

/// Applies `change` to the shown table; the status line says why it is refused.
fn report(editor: &mut Editor, change: impl FnOnce(&mut Program) -> rebof3_formats::Result<()>) {
    if let Some(message) = apply(editor, change) {
        editor.status = message;
    }
}

fn apply(editor: &mut Editor, change: impl FnOnce(&mut Program) -> rebof3_formats::Result<()>) -> Option<String> {
    let entry = editor.current()?;
    match rewritten(entry.payload(), entry.original().len(), change) {
        Ok(bytes) => {
            editor.set_payload(bytes);
            None
        }
        Err(error) => Some(format!("Script: {error}")),
    }
}

/// Writes a step field's new text.
pub fn step_changed(editor: &mut Editor, at: StepAt, part: StepPart, text: &str) -> Option<String> {
    let Ok(value) = text.trim().parse::<u8>() else { return Some("Script: a number from 0 to 255".into()) };
    let entry = editor.current()?;
    let program = Program::decode(entry.payload()).ok()?;
    let step = *program.stream(at.section, at.record)?.animation.steps.get(at.step)?;
    let op = match (Op::of(step), part) {
        (Op::Show { frame, .. }, StepPart::Ticks) if value < JUMP => Op::Show { ticks: value, frame },
        (Op::Show { .. }, StepPart::Ticks) => {
            return Some("Script: ticks run from 0 to 127; 128 and up make a jump".into());
        }
        (Op::Show { ticks, .. }, StepPart::Frame) => Op::Show { ticks, frame: value },
        (Op::Jump { code, .. }, StepPart::Target) => Op::Jump { code, target: value },
        (Op::Jump { target, .. }, StepPart::Code) if value >= JUMP => Op::Jump { code: value, target },
        (Op::Jump { .. }, StepPart::Code) => return Some("Script: a jump's code runs from 128 to 255".into()),
        _ => return None,
    };
    apply(editor, |program| program.set_op(at, op))
}

/// Writes a cell field's new text.
pub fn cell_changed(editor: &mut Editor, at: CellAt, part: CellPart, text: &str) -> Option<String> {
    let entry = editor.current()?;
    let program = Program::decode(entry.payload()).ok()?;
    let cell = *program.stream(at.section, at.record)?.animation.frames.get(at.frame)?.cells.get(at.cell)?;
    let value = text.trim().parse::<i32>().ok();
    let signed = value.and_then(|value| i8::try_from(value).ok());
    let unsigned = value.and_then(|value| u8::try_from(value).ok());
    let changed = match part {
        CellPart::Size => unsigned.map(|size| Cell { size, ..cell }),
        CellPart::X => signed.map(|x| Cell { x, ..cell }),
        CellPart::Y => signed.map(|y| Cell { y, ..cell }),
        CellPart::U => unsigned.map(|u| Cell { u, ..cell }),
        CellPart::V => unsigned.map(|v| Cell { v, ..cell }),
    };
    let Some(changed) = changed else {
        let range = if matches!(part, CellPart::X | CellPart::Y) { "-128 to 127" } else { "0 to 255" };
        return Some(format!("Script: a number from {range}"));
    };
    apply(editor, |program| program.set_cell(at, changed))
}

/// The file name an export of the shown entry starts from.
fn text_name(entry: &Entry<'_>) -> String {
    let (_, file) = entry.archive.path.split();
    let stem = file.split_once('.').map_or(file, |(stem, _)| stem);
    format!("{stem}-{}.txt", entry.index)
}

fn export(editor: &mut Editor, cx: &mut Context<Editor>) {
    let Some(entry) = editor.current() else { return };
    let (name, bytes) = (text_name(&entry), text::export(entry.payload()).map_err(|error| error.to_string()));
    editor.save_file(name, bytes, cx);
}

fn import(editor: &mut Editor, cx: &mut Context<Editor>) {
    editor.open_file("Script text to import", cx, |editor, listing| {
        let Some(entry) = editor.current() else { return "No entry is shown".into() };
        match text::import(&listing, entry.original()) {
            Ok(bytes) => {
                editor.set_payload(bytes);
                "Imported the script text".into()
            }
            Err(error) => format!("Import: {error}"),
        }
    });
}

#[cfg(test)]
mod tests {
    use rebof3_formats::Format;
    use rebof3_formats::sprite::{Animation, Frame, Step};

    use super::*;

    /// A one-section table: a stream of two shows and a jump to the second, then two bytes of padding.
    fn table() -> Vec<u8> {
        let animation = Animation {
            steps: vec![Step { delay: 3, frame: 0 }, Step { delay: 5, frame: 0 }, Step { delay: JUMP, frame: 1 }],
            frames: vec![Frame { cells: vec![Cell { size: 5, x: -8, y: -16, u: 0, v: 0 }] }],
        };
        [vec![4, 0, 0, 0, 2, 0], animation.encode(), vec![0, 0]].concat()
    }

    fn at(step: usize) -> StepAt {
        StepAt { section: 0, record: 0, step }
    }

    #[test]
    fn an_operand_edit_rewrites_one_byte() {
        let payload = table();
        let edited = rewritten(&payload, payload.len(), |program| program.set_op(at(0), Op::Show { ticks: 9, frame: 0 })).expect("edits");
        let changed = payload.iter().zip(&edited).filter(|(old, new)| old != new).count();
        assert_eq!((edited.len(), changed), (payload.len(), 1));
    }

    #[test]
    fn an_append_uses_the_padding_and_a_second_is_refused() {
        let payload = table();
        let append = |bytes: &[u8]| rewritten(bytes, payload.len(), |program| program.insert_op(at(3), Op::Show { ticks: 1, frame: 0 }));
        let once = append(&payload).expect("the padding takes the step");
        assert_eq!(once.len(), payload.len());
        assert!(append(&once).is_err(), "the table would grow");
    }

    #[test]
    fn removing_a_jump_target_is_refused() {
        let payload = table();
        assert!(rewritten(&payload, payload.len(), |program| program.remove_op(at(1))).is_err());
        assert!(rewritten(&payload, payload.len(), |program| program.remove_op(at(2))).is_ok());
    }

    /// Every retail script table takes an operand edit that rewrites one byte, and the edit undone gives
    /// the stored bytes. Point `BOF3_DISC_DIR` at an extracted disc; without it the test skips.
    #[test]
    fn every_retail_script_table_takes_an_operand_edit() {
        let Some(disc) = crate::retail_support::retail_disc() else { return };
        let folder = std::env::temp_dir().join(format!("bof3-edit-script-{}", std::process::id()));
        let mut project = crate::project::Project::open(disc.as_ref(), &folder).expect("project opens");

        let formats = [Format::AreaScriptTable, Format::BattleScriptTable, Format::MagicScriptTable];
        let (mut tables, mut failures) = (0, Vec::new());
        for path in project.archives.clone() {
            let archive = project.load(&path).expect("archive loads");
            for index in 0..archive.entries().len() {
                if !formats.contains(&archive.formats[index]) {
                    continue;
                }
                tables += 1;
                let payload = archive.original(index).to_vec();
                let program = Program::decode(&payload).expect("a retail table decodes");
                let Some((section, record, step, ticks, frame)) = first_show(&program) else {
                    failures.push(format!("{}#{index}: no show op", path.0));
                    continue;
                };
                let at = StepAt { section, record, step };
                let changed = ticks ^ 1;
                let edited = rewritten(&payload, payload.len(), |program| program.set_op(at, Op::Show { ticks: changed, frame }));
                let differ = edited.as_ref().map(|bytes| bytes.iter().zip(&payload).filter(|(new, old)| new != old).count());
                if differ != Ok(1) || edited.as_ref().map(Vec::len) != Ok(payload.len()) {
                    failures.push(format!("{}#{index}: the edit rewrites more than its byte", path.0));
                    continue;
                }
                let undone = rewritten(edited.as_deref().unwrap_or_default(), payload.len(), |program| program.set_op(at, Op::Show { ticks, frame }));
                if undone.as_deref() != Ok(payload.as_slice()) {
                    failures.push(format!("{}#{index}: the undone edit differs", path.0));
                }
            }
        }

        let _ = std::fs::remove_dir_all(&folder);
        eprintln!("script tables: {} of {tables} fail", failures.len());
        assert_eq!(tables, 279, "area, battle and magic-effect script tables");
        assert!(failures.is_empty(), "{:?}", failures.iter().take(8).collect::<Vec<_>>());
    }

    /// A retail area table draws in the view, a field edit rewrites its byte, and a jump's label picks
    /// its target. Point `BOF3_DISC_DIR` at an extracted disc; without it the test skips.
    #[gpui::test]
    fn a_retail_area_table_lists_and_edits(cx: &mut gpui::TestAppContext) {
        let Some(disc) = crate::retail_support::retail_disc() else { return };
        let folder = std::env::temp_dir().join(format!("bof3-edit-script-view-{}", std::process::id()));
        let project = folder.clone();
        let (editor, cx) = cx.add_window_view(|_, cx| Editor::new(Some(disc), Some(project), cx));
        editor.update(cx, |editor, _| {
            editor.select_archive(&crate::project::ArchivePath("BIN/WORLD00/AREA001.EMI".into()));
            let formats = &editor.archive.as_ref().expect("the area loads").formats;
            let index = formats.iter().position(|format| *format == Format::AreaScriptTable);
            editor.select_entry(index.expect("the area has a script table"));
            editor.tab = crate::view::Tab::Script;
        });
        cx.update(|window, cx| window.draw(cx).clear());

        editor.update(cx, |editor, _| {
            let entry = editor.current().expect("an entry");
            let program = Program::decode(entry.payload()).expect("decodes");
            let (section, record, step, ticks, _) = first_show(&program).expect("a show op");
            editor.views.script = State { section, record, focus: None };
            let at = StepAt { section, record, step };
            let changed = (ticks ^ 1).to_string();
            assert_eq!(step_changed(editor, at, StepPart::Ticks, &changed), None);
            let entry = editor.current().expect("an entry");
            let differ = entry.payload().iter().zip(entry.original()).filter(|(new, old)| new != old).count();
            assert_eq!(differ, 1);
            assert!(step_changed(editor, at, StepPart::Ticks, "200").is_some(), "200 ticks would make a jump");
        });
        cx.update(|window, cx| window.draw(cx).clear());
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// The first show op of a table: its place, ticks and frame.
    fn first_show(program: &Program) -> Option<(usize, usize, usize, u8, u8)> {
        for (section, found) in program.sections.iter().enumerate() {
            for (record, record_found) in found.records.iter().enumerate() {
                let Some(stream) = record_found.stream() else { continue };
                for (step, op) in stream.ops().enumerate() {
                    if let Op::Show { ticks, frame } = op
                        && ticks < JUMP - 1
                    {
                        return Some((section, record, step, ticks, frame));
                    }
                }
            }
        }
        None
    }
}
