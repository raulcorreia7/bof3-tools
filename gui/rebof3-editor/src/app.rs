//! The editor window: a menu bar, the archive browser on the left, the entry views in the middle and a
//! status line at the bottom.

use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui::{self, RichText};
use rebof3_emi::classify::Format;

use crate::project::{ArchivePath, Loaded, Project};
use crate::view::{self, Tab, Views, style};

/// Default width of the archive list, in points.
const ARCHIVES_WIDTH: f32 = 220.0;
/// Default width of the entry list, in points.
const ENTRIES_WIDTH: f32 = 300.0;
/// Width of the folder fields on the welcome screen, in points.
const PATH_WIDTH: f32 = 520.0;
/// Space above the welcome heading and below its text, in points.
const WELCOME_TOP: f32 = 40.0;
const WELCOME_GAP: f32 = 24.0;

pub struct App {
    project: Option<Project>,
    disc_field: String,
    folder_field: String,
    filter: String,
    archive: Option<Arc<Loaded>>,
    entry: Option<usize>,
    tab: Tab,
    views: Views,
    status: String,
}

impl App {
    pub fn new(disc: Option<PathBuf>, folder: Option<PathBuf>) -> Self {
        let folder =
            folder.or_else(|| disc.as_ref().and_then(|disc| disc.parent()).map(|parent| parent.join("bof3-project")));
        let mut app = Self {
            project: None,
            disc_field: disc.as_ref().map(|path| path.display().to_string()).unwrap_or_default(),
            folder_field: folder.as_ref().map(|path| path.display().to_string()).unwrap_or_default(),
            filter: String::new(),
            archive: None,
            entry: None,
            tab: Tab::Info,
            views: Views::default(),
            status: "Open an extracted disc folder (the one that holds BIN/).".into(),
        };
        if disc.is_some() {
            app.open();
        }
        app
    }

    fn open(&mut self) {
        match Project::open(self.disc_field.trim().as_ref(), self.folder_field.trim().as_ref()) {
            Ok(project) => {
                self.status = format!(
                    "{} archives, {} saved edits. Disc files are read only; edits go to {}.",
                    project.archives.len(),
                    project.edit_count(),
                    project.folder.display()
                );
                self.project = Some(project);
                self.archive = None;
                self.entry = None;
            }
            Err(error) => self.status = error,
        }
    }

    fn select_archive(&mut self, path: &ArchivePath) {
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

    fn menu(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("Project", |ui| {
                let open = self.project.is_some();
                if ui.add_enabled(open, egui::Button::new("Save edits  (Ctrl+S)")).clicked() {
                    self.save();
                    ui.close();
                }
                if ui.add_enabled(open, egui::Button::new("Export rebuilt archives  (Ctrl+E)")).clicked() {
                    self.export();
                    ui.close();
                }
                ui.separator();
                if ui.button("Close project").clicked() {
                    self.project = None;
                    self.archive = None;
                    self.entry = None;
                    ui.close();
                }
            });
            ui.menu_button("Edit", |ui| {
                let (undo, redo) = self.project.as_ref().map_or((false, false), |p| (p.can_undo(), p.can_redo()));
                if ui.add_enabled(undo, egui::Button::new("Undo  (Ctrl+Z)")).clicked() {
                    self.with_project(Project::undo);
                    ui.close();
                }
                if ui.add_enabled(redo, egui::Button::new("Redo  (Ctrl+Y)")).clicked() {
                    self.with_project(Project::redo);
                    ui.close();
                }
                let key = self.archive.as_ref().zip(self.entry).map(|(archive, index)| (archive.path.clone(), index));
                let edited = key.as_ref().is_some_and(|key| self.project.as_ref().is_some_and(|p| p.is_edited(key)));
                if ui.add_enabled(edited, egui::Button::new("Revert entry to disc")).clicked() {
                    if let (Some(project), Some(key)) = (self.project.as_mut(), key) {
                        project.revert(&key);
                    }
                    ui.close();
                }
            });
            ui.menu_button("Go", |ui| {
                if ui.button("Game tables (ETC/GAME.EMI)").clicked() {
                    self.jump("BIN/ETC/GAME.EMI", 0, Tab::Tables);
                    ui.close();
                }
            });
        });
    }

    fn jump(&mut self, archive: &str, entry: usize, tab: Tab) {
        let path = ArchivePath(archive.into());
        self.select_archive(&path);
        if self.archive.is_some() {
            self.entry = Some(entry);
            self.tab = tab;
        }
    }

    fn with_project(&mut self, action: impl FnOnce(&mut Project)) {
        if let Some(project) = self.project.as_mut() {
            action(project);
        }
    }

    fn save(&mut self) {
        if let Some(project) = self.project.as_mut() {
            self.status = match project.save() {
                Ok(count) => format!("Saved {count} edited entries to {}", project.folder.join("edits").display()),
                Err(error) => format!("Save failed: {error}"),
            };
        }
    }

    fn export(&mut self) {
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

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let (save, export, undo, redo) = ctx.input_mut(|input| {
            (
                input.consume_key(egui::Modifiers::COMMAND, egui::Key::S),
                input.consume_key(egui::Modifiers::COMMAND, egui::Key::E),
                input.consume_key(egui::Modifiers::COMMAND, egui::Key::Z),
                input.consume_key(egui::Modifiers::COMMAND, egui::Key::Y),
            )
        });
        if save {
            self.save();
        }
        if export {
            self.export();
        }
        if undo {
            self.with_project(Project::undo);
        }
        if redo {
            self.with_project(Project::redo);
        }
    }

    fn welcome(&mut self, ui: &mut egui::Ui) {
        ui.add_space(WELCOME_TOP);
        ui.vertical_centered(|ui| {
            ui.heading("BOF3 Asset Editor");
            ui.label("Browse, view and edit every entry of the game's EMI archives.");
            ui.label("The disc files are never written: edits and rebuilt archives go to the project folder.");
        });
        ui.add_space(WELCOME_GAP);
        egui::Grid::new("open").num_columns(3).spacing([style::GAP, style::GAP]).show(ui, |ui| {
            ui.label("Disc folder");
            ui.add(egui::TextEdit::singleline(&mut self.disc_field).desired_width(PATH_WIDTH));
            if ui.button("Browse…").clicked()
                && let Some(folder) = rfd::FileDialog::new().set_title("Extracted disc folder").pick_folder()
            {
                if self.folder_field.is_empty()
                    && let Some(parent) = folder.parent()
                {
                    self.folder_field = parent.join("bof3-project").display().to_string();
                }
                self.disc_field = folder.display().to_string();
            }
            ui.end_row();
            ui.label("Project folder");
            ui.add(egui::TextEdit::singleline(&mut self.folder_field).desired_width(PATH_WIDTH));
            if ui.button("Browse…").clicked()
                && let Some(folder) = rfd::FileDialog::new().set_title("Project folder for edits").pick_folder()
            {
                self.folder_field = folder.display().to_string();
            }
            ui.end_row();
        });
        ui.add_space(style::GAP);
        if ui.button(RichText::new("Open").strong()).clicked() {
            self.open();
        }
    }

    fn browser(&mut self, ui: &mut egui::Ui) {
        let Some(project) = self.project.as_ref() else { return };
        ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("Filter archives").desired_width(f32::INFINITY));
        let filter = self.filter.to_ascii_uppercase();
        let mut clicked = None;
        let selected = self.archive.as_ref().map(|archive| archive.path.clone());
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            let mut folder = "";
            let mut open = true;
            for path in project.archives.iter().filter(|path| path.0.to_ascii_uppercase().contains(&filter)) {
                let (parent, file) = path.split();
                if parent != folder {
                    folder = parent;
                    let id = ui.make_persistent_id(parent);
                    let state = egui::collapsing_header::CollapsingState::load_with_default_open(
                        ui.ctx(),
                        id,
                        !filter.is_empty(),
                    );
                    open = state.is_open();
                    let response = ui.selectable_label(false, format!("{} {parent}", if open { "▾" } else { "▸" }));
                    if response.clicked() {
                        let mut state = state;
                        state.toggle(ui);
                        state.store(ui.ctx());
                    }
                }
                if open {
                    let edited = project.archive_is_edited(path);
                    let text = if edited {
                        RichText::new(format!("   ● {file}")).color(style::EDITED)
                    } else {
                        RichText::new(format!("     {file}"))
                    };
                    if ui.selectable_label(selected.as_ref() == Some(path), text).clicked() {
                        clicked = Some(path.clone());
                    }
                }
            }
        });
        if let Some(path) = clicked {
            self.select_archive(&path);
        }
    }

    fn entries(&mut self, ui: &mut egui::Ui) {
        let (Some(project), Some(archive)) = (self.project.as_ref(), self.archive.as_ref()) else { return };
        ui.strong(&archive.path.0);
        let mut clicked = None;
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            for (index, entry) in archive.entries().iter().enumerate() {
                let format = archive.formats[index];
                let edited = project.is_edited(&(archive.path.clone(), index));
                let (tag, color) = crate::theme::tag(format);
                let mut response = None;
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{tag:<3}")).monospace().strong().color(color));
                    let label =
                        format!("{index:>3}  {:<20} {:>8}", format.name(), project.payload(archive, index).len());
                    let mut text = RichText::new(label).monospace();
                    if edited {
                        text = text.color(style::EDITED);
                    } else if format == Format::Unclassified {
                        text = text.weak();
                    }
                    response = Some(ui.selectable_label(self.entry == Some(index), text));
                });
                let Some(response) = response else { continue };
                if response.clicked() {
                    clicked = Some(index);
                }
                response.on_hover_text(format!("type {}  load {:#010X}", entry.file_type, entry.ram_ptr));
            }
        });
        if let Some(index) = clicked {
            self.entry = Some(index);
            let tabs = view::tabs(archive, index, project.payload(archive, index));
            if !tabs.contains(&self.tab) {
                self.tab = tabs.get(1).copied().unwrap_or(Tab::Info);
            }
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.shortcuts(&ui.ctx().clone());
        egui::Panel::top("menu").show(ui, |ui| self.menu(ui));
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                if let Some(project) = self.project.as_ref() {
                    let dirty = if project.is_dirty() { "unsaved" } else { "saved" };
                    ui.label(RichText::new(format!("{} edits ({dirty})", project.edit_count())).monospace());
                    ui.separator();
                }
                ui.label(&self.status);
            });
        });
        if self.project.is_none() {
            egui::CentralPanel::default().show(ui, |ui| self.welcome(ui));
            return;
        }
        egui::Panel::left("archives").default_size(ARCHIVES_WIDTH).show(ui, |ui| self.browser(ui));
        if self.archive.is_some() {
            egui::Panel::left("entries").default_size(ENTRIES_WIDTH).show(ui, |ui| self.entries(ui));
        }
        egui::CentralPanel::default().show(ui, |ui| {
            let (Some(project), Some(archive), Some(index)) = (self.project.as_mut(), self.archive.clone(), self.entry)
            else {
                ui.label("Pick an archive, then an entry.");
                return;
            };
            if let Some(message) = view::show(ui, project, &archive, index, &mut self.tab, &mut self.views) {
                self.status = message;
            }
        });
    }
}
