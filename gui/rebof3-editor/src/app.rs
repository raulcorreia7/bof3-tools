//! The editor's state and what the user can do with it: open a project, pick an archive and an
//! entry, edit, undo, save and export. `window` draws it.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    Context, FocusHandle, KeyDownEvent, Keystroke, PathPromptOptions, PromptLevel, UniformListScrollHandle, Window,
};

use crate::catalog::{self, Catalog, Mode};
use crate::edit::{LineEdit, Outcome};
use crate::project::{ArchivePath, Loaded, Project};
use crate::schema;
use crate::view::{self, Entry, Tab, Views};

/// The project folder made next to the disc folder when none is named.
const PROJECT_FOLDER: &str = "bof3-project";
/// How often the window checks for a change it did not cause, such as a sound that ends.
const TICK: Duration = Duration::from_millis(100);
/// The buttons of the prompt about unsaved edits, and the positions of the two that go on.
const UNSAVED_ANSWERS: [&str; 3] = ["Save", "Discard", "Cancel"];
const SAVE_ANSWER: usize = 0;
const DISCARD_ANSWER: usize = 1;

/// A text field of the window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Field {
    Disc,
    Folder,
    ArchiveFilter,
    View(view::Field),
}

/// An open menu or chooser.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Menu {
    Project,
    Edit,
    Go,
    View(view::Menu),
}

/// One line of the archive browser.
pub enum BrowserRow {
    Folder { name: String, open: bool },
    Archive { path: ArchivePath, edited: bool },
}

pub struct Editor {
    pub project: Option<Project>,
    pub disc_field: String,
    pub folder_field: String,
    pub filter: String,
    pub archive: Option<Arc<Loaded>>,
    pub entry: Option<usize>,
    pub tab: Tab,
    pub views: Views,
    pub status: String,
    /// The field that has the keyboard, and its text.
    pub editing: Option<(Field, LineEdit)>,
    pub menu: Option<Menu>,
    /// The folders of the archive browser that are open.
    pub open_folders: HashSet<String>,
    /// Which tree the left panel shows.
    pub mode: Mode,
    /// The game catalog of the open project.
    pub catalog: Option<Catalog>,
    /// The keys of the catalog lines that are open.
    pub catalog_open: HashSet<String>,
    pub focus: FocusHandle,
    pub archive_scroll: UniformListScrollHandle,
    pub catalog_scroll: UniformListScrollHandle,
    pub entry_scroll: UniformListScrollHandle,
}

impl Editor {
    pub fn new(disc: Option<PathBuf>, folder: Option<PathBuf>, cx: &mut Context<Self>) -> Self {
        let folder =
            folder.or_else(|| disc.as_ref().and_then(|disc| disc.parent()).map(|parent| parent.join(PROJECT_FOLDER)));
        let mut editor = Self {
            project: None,
            disc_field: disc.as_ref().map(|path| path.display().to_string()).unwrap_or_default(),
            folder_field: folder.as_ref().map(|path| path.display().to_string()).unwrap_or_default(),
            filter: String::new(),
            archive: None,
            entry: None,
            tab: Tab::Info,
            views: Views::default(),
            status: "Open an extracted disc folder (the one that holds BIN/).".into(),
            editing: None,
            menu: None,
            open_folders: HashSet::new(),
            mode: Mode::Game,
            catalog: None,
            catalog_open: HashSet::new(),
            focus: cx.focus_handle(),
            archive_scroll: UniformListScrollHandle::new(),
            catalog_scroll: UniformListScrollHandle::new(),
            entry_scroll: UniformListScrollHandle::new(),
        };
        if disc.is_some() {
            editor.open();
        }

        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                let Ok(()) = this.update(cx, |editor, cx| {
                    if editor.views.tick() {
                        cx.notify();
                    }
                }) else {
                    break;
                };
            }
        })
        .detach();
        editor
    }

    pub fn open(&mut self) {
        match Project::open(self.disc_field.trim().as_ref(), self.folder_field.trim().as_ref()) {
            Ok(mut project) => {
                self.status = format!(
                    "{} archives, {} saved edits. Disc files are read only; edits go to {}.",
                    project.archives.len(),
                    project.edit_count(),
                    project.folder.display()
                );
                // The tables' reference columns read item and ability names from the game tables.
                let _ = project.load(&ArchivePath(schema::GAME.into()));
                self.mode = Mode::read(&project.folder);
                self.catalog = match catalog::scan(&project.disc, &project.archives) {
                    Ok(scanned) => Some(Catalog::build(&scanned)),
                    Err(error) => {
                        self.status = format!("The game catalog is not available: {error}");
                        None
                    }
                };
                self.catalog_open.clear();
                self.project = Some(project);
                self.archive = None;
                self.entry = None;
            }
            Err(error) => self.status = error,
        }
    }

    pub fn close(&mut self) {
        self.stop_editing();
        self.project = None;
        self.catalog = None;
        self.archive = None;
        self.entry = None;
    }

    pub fn select_archive(&mut self, path: &ArchivePath) {
        let Some(project) = self.project.as_mut() else { return };
        match project.load(path) {
            Ok(loaded) => {
                self.status = format!("{}: {} entries", path.0, loaded.entries().len());
                self.archive = Some(loaded);
                self.entry = None;
            }
            Err(error) => self.status = error,
        }
    }

    /// Selects entry `index` of the open archive, and keeps the tab when the entry has it.
    pub fn select_entry(&mut self, index: usize) {
        self.entry = Some(index);
        let Some(entry) = self.current() else { return };
        let tabs = view::tabs(&entry);
        if !tabs.contains(&self.tab) {
            self.tab = tabs.get(1).copied().unwrap_or(Tab::Info);
        }
    }

    /// Opens the game tables.
    pub fn go_to_game_tables(&mut self) {
        self.select_archive(&ArchivePath(schema::GAME.into()));
        if self.archive.is_some() {
            self.entry = Some(0);
            self.tab = Tab::Tables;
        }
    }

    /// The entry the views show.
    pub fn current(&self) -> Option<Entry<'_>> {
        let (project, archive, index) = (self.project.as_ref()?, self.archive.as_deref()?, self.entry?);
        Some(Entry { project, archive, index })
    }

    /// Replaces the payload of the shown entry; undo, save and export see the change.
    pub fn set_payload(&mut self, bytes: Vec<u8>) {
        if let (Some(project), Some(archive), Some(index)) =
            (self.project.as_mut(), self.archive.as_deref(), self.entry)
            && project.payload(archive, index) != bytes.as_slice()
        {
            project.set(archive, index, bytes);
        }
    }

    /// Rewrites the payload of entry `index` of `archive` with `rewrite`. A prompt names the entry before it
    /// opens, so the change lands there even when the selection moved meanwhile.
    pub fn rewrite_entry(
        &mut self,
        archive: &ArchivePath,
        index: usize,
        rewrite: impl FnOnce(&[u8]) -> Result<Vec<u8>, String>,
    ) -> Result<(), String> {
        self.rewrite_entries(archive, &[index], |payloads| Ok(vec![rewrite(payloads[0])?]))
    }

    /// Rewrites the payloads of entries `indices` of `archive` with `rewrite`, as one undo step. `rewrite`
    /// gets the payloads in the order of `indices` and returns the new ones in the same order.
    pub fn rewrite_entries(
        &mut self,
        archive: &ArchivePath,
        indices: &[usize],
        rewrite: impl FnOnce(Vec<&[u8]>) -> Result<Vec<Vec<u8>>, String>,
    ) -> Result<(), String> {
        let project = self.project.as_mut().ok_or("no project is open")?;
        let loaded = project.load(archive)?;
        let payloads = indices.iter().map(|&index| project.payload(&loaded, index)).collect();
        let rewritten = rewrite(payloads)?;
        project.set_together(&loaded, indices.iter().copied().zip(rewritten).collect());
        Ok(())
    }

    pub fn is_entry_edited(&self) -> bool {
        let (Some(project), Some(archive), Some(index)) = (&self.project, &self.archive, self.entry) else {
            return false;
        };
        project.is_edited(&(archive.path.clone(), index))
    }

    pub fn revert_entry(&mut self) {
        if let (Some(project), Some(archive), Some(index)) = (self.project.as_mut(), &self.archive, self.entry) {
            project.revert(&(archive.path.clone(), index));
        }
    }

    /// Takes back the last step. An open field closes first, so its text cannot write the entry again.
    pub fn undo(&mut self) {
        self.stop_editing();
        if let Some(project) = self.project.as_mut() {
            project.undo();
        }
    }

    pub fn redo(&mut self) {
        self.stop_editing();
        if let Some(project) = self.project.as_mut() {
            project.redo();
        }
    }

    /// Starts an undo step that collects every change of the gesture or field that begins now.
    pub fn begin_step(&mut self) {
        if let Some(project) = self.project.as_mut() {
            project.begin_step();
        }
    }

    /// Ends the open undo step.
    pub fn end_step(&mut self) {
        if let Some(project) = self.project.as_mut() {
            project.end_step();
        }
    }

    /// Whether the project holds edits the project folder does not.
    pub fn has_unsaved_edits(&self) -> bool {
        self.project.as_ref().is_some_and(Project::is_dirty)
    }

    /// Runs `then` when no edit would be lost: at once when every edit is saved, otherwise after the user
    /// saves or discards them. Cancel, or a save that fails, leaves everything as it was.
    pub fn when_saved(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: fn(&mut Self, &mut Window, &mut Context<Self>),
    ) {
        if !self.has_unsaved_edits() {
            then(self, window, cx);
            return;
        }
        let answer = window.prompt(
            PromptLevel::Warning,
            "Save the edits before closing?",
            Some("Discard drops every edit made since the last save."),
            &UNSAVED_ANSWERS,
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let Ok(answer) = answer.await else { return };
            let _ = this.update_in(cx, |editor, window, cx| {
                match answer {
                    SAVE_ANSWER => {
                        editor.save();
                        if editor.has_unsaved_edits() {
                            return;
                        }
                    }
                    DISCARD_ANSWER => {}
                    _ => return,
                }
                then(editor, window, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Whether the window may close now. With unsaved edits it asks first, and closes itself after a save
    /// or a discard.
    pub fn should_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.has_unsaved_edits() {
            return true;
        }
        self.when_saved(window, cx, |_, window, _| window.remove_window());
        false
    }

    pub fn save(&mut self) {
        if let Some(project) = self.project.as_mut() {
            self.status = match project.save() {
                Ok(count) => format!("Saved {count} edited entries to {}", project.folder.join("edits").display()),
                Err(error) => format!("Save failed: {error}"),
            };
        }
    }

    pub fn export(&mut self) {
        if let Some(project) = self.project.as_mut() {
            self.status = match project.export() {
                Ok(files) if files.is_empty() => "Nothing to export: no entry is edited.".into(),
                Ok(files) => format!(
                    "Rebuilt and verified {} archives in {}",
                    files.len(),
                    project.folder.join("disc").display()
                ),
                Err(error) => format!("Export failed: {error}"),
            };
        }
    }

    /// The archive browser's lines: folders, and the archives of each open folder. A filter opens
    /// every folder that has a match.
    pub fn browser_rows(&self) -> Vec<BrowserRow> {
        let Some(project) = self.project.as_ref() else { return Vec::new() };
        let filter = self.filter.to_ascii_uppercase();
        let mut rows = Vec::new();
        let mut folder = "";
        let mut open = true;

        for path in project.archives.iter().filter(|path| path.0.to_ascii_uppercase().contains(&filter)) {
            let (parent, _) = path.split();
            if parent != folder {
                folder = parent;
                open = !filter.is_empty() || self.open_folders.contains(parent);
                rows.push(BrowserRow::Folder { name: parent.to_string(), open });
            }
            if open {
                rows.push(BrowserRow::Archive { path: path.clone(), edited: project.archive_is_edited(path) });
            }
        }
        rows
    }

    /// The lines of the game catalog under the panel's filter.
    pub fn catalog_lines(&self) -> Vec<catalog::Line> {
        self.catalog.as_ref().map_or_else(Vec::new, |catalog| catalog.lines(&self.filter, &self.catalog_open))
    }

    /// Opens or shuts a catalog line.
    pub fn toggle_catalog(&mut self, key: &str) {
        if !self.catalog_open.remove(key) {
            self.catalog_open.insert(key.to_string());
        }
    }

    /// Switches the panel between the disc tree and the game catalog, and remembers the choice.
    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
        if let Some(project) = self.project.as_ref() {
            mode.write(&project.folder);
        }
    }

    /// Selects the entry a catalog line names, or only its archive when the line is an archive.
    pub fn select_catalog(&mut self, archive: &ArchivePath, entry: Option<usize>) {
        if self.archive.as_ref().map(|loaded| &loaded.path) != Some(archive) {
            self.select_archive(archive);
        }
        if let Some(entry) = entry.filter(|_| self.archive.is_some()) {
            self.select_entry(entry);
        }
    }

    pub fn toggle_folder(&mut self, name: &str) {
        if !self.open_folders.remove(name) {
            self.open_folders.insert(name.to_string());
        }
    }

    /// Gives `field` the keyboard, starting from `text`.
    pub fn start_editing(&mut self, field: Field, text: String) {
        self.stop_editing();
        self.menu = None;
        self.editing = Some((field, LineEdit::new(text)));
        self.begin_step();
    }

    /// Takes the keyboard from the open field, which ends its undo step.
    pub fn stop_editing(&mut self) {
        if self.editing.take().is_some() {
            self.end_step();
        }
    }

    /// Asks for a folder and puts it in `field`. The disc folder also names a project folder next
    /// to it when none is set.
    pub fn browse(&mut self, field: Field, cx: &mut Context<Self>) {
        let prompt = match field {
            Field::Disc => "Extracted disc folder",
            _ => "Project folder for edits",
        };
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(prompt.into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else { return };
            let Some(folder) = paths.into_iter().next() else { return };
            let _ = this.update(cx, |editor, cx| {
                if field == Field::Disc {
                    if editor.folder_field.is_empty()
                        && let Some(parent) = folder.parent()
                    {
                        editor.folder_field = parent.join(PROJECT_FOLDER).display().to_string();
                    }
                    editor.disc_field = folder.display().to_string();
                } else {
                    editor.folder_field = folder.display().to_string();
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Asks where to save `name` and writes `bytes` there. The status line reports the result.
    pub fn save_file(&mut self, name: String, bytes: Result<Vec<u8>, String>, cx: &mut Context<Self>) {
        let bytes = match bytes {
            Ok(bytes) => bytes,
            Err(error) => {
                self.status = error;
                return;
            }
        };
        let folder = self.project.as_ref().map_or_else(|| PathBuf::from("."), |project| project.folder.clone());
        let picked = cx.prompt_for_new_path(&folder, Some(&name));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = picked.await else { return };
            let status = match std::fs::write(&path, bytes) {
                Ok(()) => format!("Wrote {}", path.display()),
                Err(error) => format!("{}: {error}", path.display()),
            };
            let _ = this.update(cx, |editor, cx| {
                editor.status = status;
                cx.notify();
            });
        })
        .detach();
    }

    /// Asks for a file and hands its bytes to `then`. The status line shows what `then` returns, or why the
    /// file cannot be read.
    pub fn open_file(
        &mut self,
        prompt: &'static str,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, Vec<u8>) -> String + 'static,
    ) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(prompt.into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            let read = std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()));
            let _ = this.update(cx, |editor, cx| {
                editor.status = match read {
                    Ok(bytes) => then(editor, bytes),
                    Err(error) => error,
                };
                cx.notify();
            });
        })
        .detach();
    }

    /// The key map: the shortcuts, then the field that has the keyboard, then the shown view.
    pub fn on_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if self.shortcut(keystroke) || self.edit_key(keystroke, cx) || self.view_key(keystroke) {
            cx.stop_propagation();
            cx.notify();
        }
    }

    fn shortcut(&mut self, keystroke: &Keystroke) -> bool {
        let command = keystroke.modifiers.control || keystroke.modifiers.platform;
        if !command {
            return false;
        }
        match keystroke.key.as_str() {
            "s" => self.save(),
            "e" => self.export(),
            "z" => self.undo(),
            "y" => self.redo(),
            _ => return false,
        }
        true
    }

    fn edit_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let Some((field, edit)) = self.editing.as_mut() else {
            if keystroke.key == "escape" && self.menu.take().is_some() {
                return true;
            }
            return false;
        };
        let field = *field;
        let multiline = matches!(field, Field::View(field) if field.is_multiline());
        let pasted = cx.read_from_clipboard().and_then(|item| item.text());

        match edit.key(keystroke, multiline, pasted) {
            Outcome::Kept => {}
            Outcome::Changed => {
                let text = edit.text.clone();
                self.field_changed(field, text);
            }
            Outcome::Submit => {
                let text = edit.text.clone();
                self.stop_editing();
                self.field_submitted(field, &text);
            }
            Outcome::Cancel => self.stop_editing(),
        }
        true
    }

    fn field_changed(&mut self, field: Field, text: String) {
        match field {
            Field::Disc => self.disc_field = text,
            Field::Folder => self.folder_field = text,
            Field::ArchiveFilter => self.filter = text,
            Field::View(field) => {
                if let Some(message) = view::field_changed(self, field, &text) {
                    self.status = message;
                }
            }
        }
    }

    fn field_submitted(&mut self, field: Field, text: &str) {
        match field {
            Field::Disc | Field::Folder => self.open(),
            Field::ArchiveFilter => {}
            Field::View(field) => {
                if let Some(message) = view::field_submitted(self, field, text) {
                    self.status = message;
                }
            }
        }
    }

    fn view_key(&mut self, keystroke: &Keystroke) -> bool {
        if self.current().is_none() {
            return false;
        }
        view::key(self, keystroke)
    }
}

#[cfg(test)]
mod tests {
    use gpui::{Entity, TestAppContext, VisualTestContext};
    use rebof3_editor::fixture::{ARCHIVE, Disc, Entry as DiscEntry};

    use super::*;

    /// The bytes of the one entry the tests edit.
    const STORED: [u8; 4] = [1, 2, 3, 4];

    /// An editor on a one-entry disc, with that entry shown and its hex inspector's `u8` field open.
    fn editing<'a>(name: &str, cx: &'a mut TestAppContext) -> (Disc, Entity<Editor>, &'a mut VisualTestContext) {
        let disc = Disc::new(name, &[DiscEntry { file_type: 0, ram_ptr: 0, bytes: &STORED }]);
        let (disc_folder, project_folder) = (disc.disc(), disc.project());
        let (editor, cx) = cx.add_window_view(|_, cx| Editor::new(Some(disc_folder), Some(project_folder), cx));
        editor.update(cx, |editor, _| {
            editor.select_archive(&ArchivePath(ARCHIVE.into()));
            editor.select_entry(0);
            editor.start_editing(Field::View(view::Field::Inspector(0)), String::new());
        });
        (disc, editor, cx)
    }

    fn type_keys(editor: &Entity<Editor>, keys: &str, cx: &mut VisualTestContext) {
        editor.update(cx, |editor, cx| {
            for key in keys.split(' ') {
                let mut keystroke = Keystroke::parse(key).expect("a key");
                if key.chars().count() == 1 {
                    keystroke.key_char = Some(key.into());
                }
                editor.edit_key(&keystroke, cx);
            }
        });
    }

    fn first_byte(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> u8 {
        editor.read_with(cx, |editor, _| editor.current().expect("an entry").payload()[0])
    }

    #[gpui::test]
    fn a_typed_field_undoes_as_one_step(cx: &mut TestAppContext) {
        let (_disc, editor, cx) = editing("field-step", cx);
        type_keys(&editor, "1 2 7 enter", cx);
        assert_eq!(first_byte(&editor, cx), 127);

        editor.update(cx, |editor, _| editor.undo());
        assert_eq!(first_byte(&editor, cx), STORED[0]);
        assert!(editor.read_with(cx, |editor, _| !editor.project.as_ref().expect("a project").can_undo()));
    }

    #[gpui::test]
    fn undo_closes_an_open_field(cx: &mut TestAppContext) {
        let (_disc, editor, cx) = editing("undo-closes", cx);
        type_keys(&editor, "9", cx);

        editor.update(cx, |editor, _| editor.undo());
        assert!(editor.read_with(cx, |editor, _| editor.editing.is_none()));

        type_keys(&editor, "8", cx);
        assert_eq!(first_byte(&editor, cx), STORED[0], "the closed field writes nothing");
    }

    #[gpui::test]
    fn the_game_panel_opens_an_entry_and_remembers_its_mode(cx: &mut TestAppContext) {
        let (_disc, editor, cx) = editing("catalog-panel", cx);
        editor.update(cx, |editor, _| {
            assert_eq!(editor.mode, Mode::Game);
            let lines = editor.catalog_lines();
            assert!(lines.iter().all(|line| line.depth == 0), "the groups start shut");
            let key = lines[0].key.clone();
            editor.toggle_catalog(&key);
            assert!(editor.catalog_lines().len() > lines.len());

            editor.archive = None;
            editor.select_catalog(&ArchivePath(ARCHIVE.into()), Some(0));
            assert_eq!(editor.entry, Some(0));
            editor.set_mode(Mode::Disc);
        });
        cx.update(|window, cx| window.draw(cx).clear());
        editor.update(cx, |editor, _| {
            let folder = editor.project.as_ref().expect("a project").folder.clone();
            assert_eq!(Mode::read(&folder), Mode::Disc);
        });
    }

    #[gpui::test]
    fn unsaved_edits_keep_the_window_open(cx: &mut TestAppContext) {
        let (_disc, editor, cx) = editing("close", cx);
        assert!(editor.update_in(cx, |editor, window, cx| editor.should_close(window, cx)));

        type_keys(&editor, "5 enter", cx);
        assert!(!editor.update_in(cx, |editor, window, cx| editor.should_close(window, cx)));
    }
}
