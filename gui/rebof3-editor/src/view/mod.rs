//! The views of one entry. Each view reads the entry's current payload and writes a whole new
//! payload back through [`Editor::set_payload`], so undo, save and export see every view's edits the
//! same way.

mod cells;
mod decoded;
mod hex;
mod info;
mod map;
mod message;
mod palette;
mod scene;
mod sound;
mod sprite;
mod tables;
mod text;
mod texture;
mod vram;

use gpui::{AnyElement, Context, Div, ElementId, Keystroke, Stateful, div, prelude::*, px};
use rebof3_emi::classify::Format;

use crate::app::{Editor, Field as WindowField};
use crate::edit::LineEdit;
use crate::project::{ArchivePath, Loaded, Project};
use crate::widgets::{GAP_SMALL, field, separator, tab};

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
    Map,
    Sprite,
    Cells,
    Vram,
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
            Tab::Map => "Map",
            Tab::Sprite => "Sprite bank",
            Tab::Cells => "Cell grid",
            Tab::Vram => "VRAM",
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
    /// The text view's filter.
    TextFilter,
    /// The string that starts at this offset.
    Text(usize),
    /// The tables view's filter.
    TableFilter,
    /// A name or number cell of a table.
    Cell { row: usize, column: usize },
    /// A corner height of the map view's picked cell.
    MapCorner(usize),
    /// A header field of sequence `index` of the sound view's package.
    SequenceHeader { index: usize, part: sound::Part },
    /// The note the sound view plays tone `slot` of program block `block` at.
    ToneNote { block: u8, slot: u8 },
}

impl Field {
    /// Whether Shift+Enter breaks a line in the field.
    pub fn is_multiline(self) -> bool {
        matches!(self, Field::Text(_))
    }
}

/// A chooser that a view opens.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Menu {
    /// The texture view's palette rows.
    Colors,
    /// The bits or choices of a table cell.
    Cell { row: usize, column: usize },
}

/// The state each view keeps between frames.
#[derive(Default)]
pub struct Views {
    pub hex: hex::State,
    pub texture: texture::State,
    pub palette: palette::State,
    pub tables: tables::State,
    pub text: text::State,
    pub sound: sound::State,
    pub decoded: decoded::State,
    pub map: map::State,
    pub sprite: sprite::State,
    pub cells: cells::State,
    pub vram: vram::State,
}

impl Views {
    /// Advances what changes without input. Returns true when the window should redraw.
    pub fn tick(&mut self) -> bool {
        self.sound.tick()
    }
}

/// Resets the state of a view whose entry changed, before the window draws it.
pub fn prepare(editor: &mut Editor) {
    let Some(entry) = editor.current() else { return };
    if editor.tab == Tab::Vram && tabs(&entry).contains(&Tab::Vram) {
        vram::prepare(editor);
    }
    let Some(entry) = editor.current() else { return };
    match entry.format() {
        Format::VramUpload => texture::prepare(editor),
        Format::AreaMapBundle if editor.tab == Tab::Map => map::prepare(editor),
        Format::Lz77Blob | Format::CharacterBlob if editor.tab == Tab::Sprite => sprite::prepare(editor),
        _ => {}
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

    /// The current payload of entry `index` of `archive`, when the project has read that archive.
    pub fn elsewhere(&self, archive: &str, index: usize) -> Option<&[u8]> {
        let loaded = self.project.loaded(&ArchivePath(archive.to_string()))?;
        (index < loaded.entries().len()).then(|| self.project.payload(loaded, index))
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
        Format::VramUpload => tabs.extend([Tab::Texture, Tab::Vram]),
        Format::ClutBank => tabs.extend([Tab::Palette, Tab::Vram]),
        Format::VabHeader | Format::VabBody | Format::SepPackage => tabs.push(Tab::Sound),
        Format::AreaMapBundle => tabs.push(Tab::Map),
        Format::Lz77Blob | Format::CharacterBlob => tabs.push(Tab::Sprite),
        Format::AreaCellGrid => tabs.push(Tab::Cells),
        _ => {}
    }
    if tables::has_tables(entry) {
        tabs.push(Tab::Tables);
    }
    if text::has_text(entry.payload(), entry.load_arg()) {
        tabs.push(Tab::Text);
    }
    if decoded::decodes(entry.format()) {
        tabs.push(Tab::Decoded);
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
        Tab::Tables => tables::show(editor, &entry, cx),
        Tab::Text => text::show(editor, &entry, cx),
        Tab::Sound => sound::show(editor, &entry, cx),
        Tab::Decoded => decoded::show(editor, &entry, cx),
        Tab::Map => map::show(editor, &entry, cx),
        Tab::Sprite => sprite::show(editor, &entry, cx),
        Tab::Cells => cells::show(editor, &entry, cx),
        Tab::Vram => vram::show(editor, &entry, cx),
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

/// The text field of `which`: it shows `text`, or `hint` while `text` is empty, and a click gives it the
/// keyboard, starting from `text`.
pub fn input(
    editor: &Editor,
    id: impl Into<ElementId>,
    which: Field,
    text: &str,
    hint: &str,
    width: Option<f32>,
    cx: &mut Context<Editor>,
) -> Stateful<Div> {
    let draft = text.to_string();
    field(id, text, hint, editing(editor, which), width).on_click(cx.listener(move |editor, _, _, cx| {
        editor.start_editing(WindowField::View(which), draft.clone());
        cx.notify();
    }))
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
        Field::TextFilter => {
            editor.views.text.filter = text.to_string();
            None
        }
        Field::Text(start) => text::string_changed(editor, start, text),
        Field::TableFilter => {
            editor.views.tables.filter = text.to_string();
            None
        }
        Field::Cell { row, column } => tables::cell_changed(editor, row, column, text),
        Field::MapCorner(corner) => map::corner_changed(editor, corner, text),
        Field::SequenceHeader { index, part } => sound::header_changed(editor, index, part, text),
        Field::ToneNote { block, slot } => sound::note_changed(editor, block, slot, text),
    }
}

/// Applies Enter in a field. Returns a status message.
pub fn field_submitted(editor: &mut Editor, field: Field, text: &str) -> Option<String> {
    match field {
        Field::Goto => hex::go_to(editor, text),
        _ => None,
    }
}

/// The keys of the shown view, when no field has the keyboard. Returns true when the key was used.
pub fn key(editor: &mut Editor, keystroke: &Keystroke) -> bool {
    match editor.tab {
        Tab::Hex => hex::key(editor, keystroke),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;
    use rebof3_editor::fixture::{ARCHIVE, Disc, Entry as DiscEntry};
    use rebof3_emi::classify::Format;

    use super::*;
    use crate::project::ArchivePath;
    use crate::schema;

    /// Entry types and load arguments that the classifier reads as each format but the reserve
    /// (`rebof3_emi::classify`).
    const PLACES: [(u16, u32); 20] = [
        (3, 0x8000_0000), // VRAM texture upload
        (0, 0x8003_3800), // CLUT bank
        (6, 0),           // VAB header
        (7, 0),           // VAB body
        (10, 0),          // SEP sequence package
        (1, 0),           // LZ77 sprite bank
        (0, 0x8003_b800), // raw sprite bank
        (0, 0x8001_0000), // area message bank
        (0, 0x8003_2000), // area cell grid
        (0, 0x800d_3800), // area script bank
        (0, 0x800f_0800), // battle script bank
        (0, 0x800e_3800), // area actor definitions
        (0, 0x800e_4000), // area encounters
        (0, 0x8010_4000), // area map bundle
        (0, 0x8011_7000), // auxiliary module window
        (0, 0x8001_a000), // shared resource table
        (8, 0),           // cue table
        (0, 0x800c_3800), // magic-effect script bank
        (0, 0x8009_6800), // code image
        (2, 0),           // unclassified
    ];
    /// The load argument of `ETC/GAME.EMI` entry 0, the code image that holds the game tables.
    const GAME_CODE: u32 = 0x8019_5800;
    /// A payload shorter than any header.
    const SHORT: [u8; 3] = [0xFF, 0x01, 0x80];
    /// The size of the noise payload, and the multiplier and increment of its linear congruential generator.
    const NOISE_BYTES: usize = 4096;
    const NOISE_MULTIPLIER: u32 = 1_103_515_245;
    const NOISE_INCREMENT: u32 = 12_345;

    fn noise() -> Vec<u8> {
        let mut state = 1u32;
        (0..NOISE_BYTES)
            .map(|_| {
                state = state.wrapping_mul(NOISE_MULTIPLIER).wrapping_add(NOISE_INCREMENT);
                (state >> 16) as u8
            })
            .collect()
    }

    /// Shows every view of every format over an empty, a short and a noise payload, and draws the window
    /// each time: a view reports a bad payload, and none panics. The game and party tables get their own
    /// archives, since their sheets apply to one entry of one archive.
    #[gpui::test]
    fn every_view_survives_a_bad_payload(cx: &mut TestAppContext) {
        let noise = noise();
        for (name, payload) in [("empty", &[][..]), ("short", &SHORT[..]), ("noise", &noise[..])] {
            let entries: Vec<DiscEntry<'_>> =
                PLACES.iter().map(|&(file_type, ram_ptr)| DiscEntry { file_type, ram_ptr, bytes: payload }).collect();
            let game = [DiscEntry { file_type: 0, ram_ptr: GAME_CODE, bytes: payload }];
            let start: Vec<DiscEntry<'_>> = (0..=schema::START_PARTY_ENTRY)
                .map(|_| DiscEntry { file_type: 0, ram_ptr: 0, bytes: payload })
                .collect();
            let archives = [(ARCHIVE, &entries[..]), (schema::GAME, &game[..]), (schema::START, &start[..])];
            let disc = Disc::with_archives(&format!("bad-{name}"), &archives);
            let (disc_folder, project_folder) = (disc.disc(), disc.project());
            let (editor, cx) = cx.add_window_view(|_, cx| Editor::new(Some(disc_folder), Some(project_folder), cx));

            for (archive, entries) in archives {
                let formats = editor.update(cx, |editor, _| {
                    editor.select_archive(&ArchivePath(archive.into()));
                    editor.archive.as_ref().expect("the archive loads").formats.clone()
                });
                if name == "noise" && archive == ARCHIVE {
                    for format in Format::ALL.iter().filter(|format| **format != Format::Reserve) {
                        assert!(formats.contains(format), "{format:?} has no entry");
                    }
                }

                for index in 0..entries.len() {
                    let shown = editor.update(cx, |editor, _| {
                        editor.select_entry(index);
                        tabs(&editor.current().expect("an entry"))
                    });
                    for tab in shown {
                        editor.update(cx, |editor, _| editor.tab = tab);
                        cx.update(|window, cx| window.draw(cx).clear());
                    }
                }
            }
        }
    }

    /// An area map draws textured from its own scene's VRAM. Point `BOF3_DISC_DIR` at an extracted
    /// disc; without it the test skips.
    #[gpui::test]
    fn a_retail_area_map_draws_textured(cx: &mut TestAppContext) {
        let Some(disc) = std::env::var_os("BOF3_DISC_DIR").map(std::path::PathBuf::from) else {
            return eprintln!("skipping: set BOF3_DISC_DIR");
        };
        let folder = std::env::temp_dir().join(format!("bof3-edit-map-{}", std::process::id()));
        let project = folder.clone();
        let (editor, cx) = cx.add_window_view(|_, cx| Editor::new(Some(disc), Some(project), cx));
        editor.update(cx, |editor, _| {
            editor.select_archive(&ArchivePath("BIN/WORLD00/AREA001.EMI".into()));
            let index = editor
                .archive
                .as_ref()
                .expect("the area loads")
                .formats
                .iter()
                .position(|format| *format == Format::AreaMapBundle);
            editor.select_entry(index.expect("the area has a map bundle"));
            editor.tab = Tab::Map;
        });
        cx.update(|window, cx| window.draw(cx).clear());
        let summary = editor.update(cx, |editor, _| editor.views.map.textured_summary());
        let summary = summary.expect("the map is textured");
        assert!(summary.contains("Field: AREA001") && summary.contains(", 0 whose page"), "{summary}");
        let _ = std::fs::remove_dir_all(&folder);
    }
}
