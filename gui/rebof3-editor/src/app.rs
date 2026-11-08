//! The editor's state and what the user can do with it: open a project, pick an archive and an
//! entry, edit, undo, save and export. `window` draws it.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui::{Context, FocusHandle, KeyDownEvent, Keystroke, PathPromptOptions, UniformListScrollHandle, Window};

use crate::edit::{LineEdit, Outcome};
use crate::project::{ArchivePath, Loaded, Project};
use crate::schema;
use crate::view::{self, Entry, Tab, Views};

/// The project folder made next to the disc folder when none is named.
const PROJECT_FOLDER: &str = "bof3-project";
/// How often the window checks for a change it did not cause, such as a sound that ends.
const TICK: Duration = Duration::from_millis(100);

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
    pub focus: FocusHandle,
    pub archive_scroll: UniformListScrollHandle,
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
            focus: cx.focus_handle(),
            archive_scroll: UniformListScrollHandle::new(),
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
                self.project = Some(project);
                self.archive = None;
                self.entry = None;
            }
            Err(error) => self.status = error,
        }
    }

    pub fn close(&mut self) {
        self.project = None;
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

    pub fn undo(&mut self) {
        if let Some(project) = self.project.as_mut() {
            project.undo();
        }
    }

    pub fn redo(&mut self) {
        if let Some(project) = self.project.as_mut() {
            project.redo();
        }
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

    pub fn toggle_folder(&mut self, name: &str) {
        if !self.open_folders.remove(name) {
            self.open_folders.insert(name.to_string());
        }
    }

    /// Gives `field` the keyboard, starting from `text`.
    pub fn start_editing(&mut self, field: Field, text: String) {
        self.menu = None;
        self.editing = Some((field, LineEdit::new(text)));
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
                self.editing = None;
                self.field_submitted(field, &text);
            }
            Outcome::Cancel => self.editing = None,
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
