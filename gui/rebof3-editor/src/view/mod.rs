//! The views of one entry. Each view reads the entry's current payload and writes a whole new
//! payload back through [`Editor::set_payload`], so undo, save and export see every view's edits the
//! same way.

mod hex;
mod info;
mod palette;
mod texture;

use gpui::{AnyElement, Context, Keystroke, div, prelude::*, px};
use rebof3_emi::classify::Format;

use crate::app::{Editor, Field as WindowField};
use crate::edit::LineEdit;
use crate::project::{Loaded, Project};
use crate::widgets::{GAP_SMALL, separator, tab};

/// One view of an entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tab {
    Info,
    Hex,
    Texture,
    Palette,
}

impl Tab {
    fn label(self) -> &'static str {
        match self {
            Tab::Info => "Info",
            Tab::Hex => "Hex",
            Tab::Texture => "Texture",
            Tab::Palette => "Palette",
        }
    }
}

/// A text field of a view.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Field {
    /// The hex view's go-to offset.
    Goto,
    /// A row of the hex view's value inspector.
    Inspector(usize),
}

impl Field {
    /// Whether Shift+Enter breaks a line in the field.
    pub fn is_multiline(self) -> bool {
        false
    }
}

/// A chooser that a view opens.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Menu {
    /// The texture view's palette rows.
    Colors,
}

/// The state each view keeps between frames.
#[derive(Default)]
pub struct Views {
    pub hex: hex::State,
    pub texture: texture::State,
    pub palette: palette::State,
}

impl Views {
    /// Advances what changes without input. Returns true when the window should redraw.
    pub fn tick(&mut self) -> bool {
        false
    }
}

/// Resets the state of a view whose entry changed, before the window draws it.
pub fn prepare(editor: &mut Editor) {
    let Some(entry) = editor.current() else { return };
    if entry.format() == Format::VramUpload {
        let mut texture = std::mem::take(&mut editor.views.texture);
        if let Some(entry) = editor.current() {
            texture.prepare(&entry);
        }
        editor.views.texture = texture;
    }
}

/// What a view reads: the project, the archive and the entry index.
pub struct Entry<'a> {
    pub project: &'a Project,
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

    /// The payload as it is on the disc.
    pub fn original(&self) -> &[u8] {
        self.archive.original(self.index)
    }

    /// The payload of another entry of the same archive.
    pub fn sibling(&self, index: usize) -> &[u8] {
        self.project.payload(self.archive, index)
    }
}

/// The views that apply to an entry, Info and Hex first.
pub fn tabs(entry: &Entry<'_>) -> Vec<Tab> {
    let mut tabs = vec![Tab::Info, Tab::Hex];
    match entry.format() {
        Format::VramUpload => tabs.push(Tab::Texture),
        Format::ClutBank => tabs.push(Tab::Palette),
        _ => {}
    }
    tabs
}

/// The tab bar and the selected view.
pub fn show(editor: &Editor, cx: &mut Context<Editor>) -> AnyElement {
    let Some(entry) = editor.current() else { return div().into_any_element() };
    let available = tabs(&entry);
    let shown = if available.contains(&editor.tab) { editor.tab } else { Tab::Info };

    let mut bar = div().flex().flex_none().gap(px(GAP_SMALL));
    for candidate in available {
        bar = bar.child(tab(candidate.label(), candidate.label(), candidate == shown).on_click(cx.listener(
            move |editor, _, _, cx| {
                editor.tab = candidate;
                cx.notify();
            },
        )));
    }

    let body = match shown {
        Tab::Info => info::show(&entry),
        Tab::Hex => hex::show(editor, &entry, cx),
        Tab::Texture => texture::show(editor, &entry, cx),
        Tab::Palette => palette::show(editor, &entry, cx),
    };
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .gap(px(GAP_SMALL))
        .child(bar)
        .child(separator(false))
        .child(body)
        .into_any_element()
}

/// The line edit of `field` while it has the keyboard.
pub fn editing(editor: &Editor, field: Field) -> Option<&LineEdit> {
    editor.editing.as_ref().filter(|(which, _)| *which == WindowField::View(field)).map(|(_, edit)| edit)
}

/// Whether the chooser `menu` is open.
pub fn editing_menu(editor: &Editor, menu: Menu) -> bool {
    editor.menu == Some(crate::app::Menu::View(menu))
}

/// Applies a changed field. Returns a status message when the text cannot be written.
pub fn field_changed(editor: &mut Editor, field: Field, text: &str) -> Option<String> {
    match field {
        Field::Goto => {
            editor.views.hex.goto = text.to_string();
            None
        }
        Field::Inspector(row) => hex::inspector_changed(editor, row, text),
    }
}

/// Applies Enter in a field. Returns a status message.
pub fn field_submitted(editor: &mut Editor, field: Field, text: &str) -> Option<String> {
    match field {
        Field::Goto => hex::go_to(editor, text),
        Field::Inspector(_) => None,
    }
}

/// The keys of the shown view, when no field has the keyboard. Returns true when the key was used.
pub fn key(editor: &mut Editor, keystroke: &Keystroke) -> bool {
    match editor.tab {
        Tab::Hex => hex::key(editor, keystroke),
        _ => false,
    }
}
