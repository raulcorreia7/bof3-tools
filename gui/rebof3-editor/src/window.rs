//! Drawing the editor window: a menu bar, the archive browser on the left, the entry list next to
//! it, the entry views in the middle and a status line at the bottom.

use gpui::{
    AnyElement, ClickEvent, Context, Div, FontWeight, IntoElement, Render, Window, anchored, deferred, div, prelude::*,
    px, uniform_list,
};
use rebof3_emi::classify::Format;

use crate::app::{BrowserRow, Editor, Field, Menu};
use crate::fonts;
use crate::theme::{self, BACKGROUND, BORDER, EDITED, MUTED, PANEL, TEXT, color};
use crate::view;
use crate::widgets::{
    CONTROL_HEIGHT, GAP, GAP_SECTION, GAP_SMALL, ROW_HEIGHT, button, field, menu_item, menu_panel, mono, muted,
    selectable, separator, strong, tip,
};

/// Width of the archive list and of the entry list, in pixels.
const ARCHIVES_WIDTH: f32 = 220.0;
const ENTRIES_WIDTH: f32 = 400.0;
/// Width of the folder fields on the welcome screen, and of their labels, in pixels.
const PATH_WIDTH: f32 = 520.0;
const PATH_LABEL_WIDTH: f32 = 110.0;
/// Space above the welcome heading and below its text, in pixels.
const WELCOME_TOP: f32 = 40.0;
const WELCOME_GAP: f32 = 24.0;
/// Side of the icon in the menu bar and on the welcome screen, in pixels.
const MENU_ICON: f32 = 18.0;
const WELCOME_ICON: f32 = 64.0;
/// Columns of an entry row: the format tag, the index and the format name.
const TAG_COLUMNS: usize = 3;
const INDEX_COLUMNS: usize = 3;
const NAME_COLUMNS: usize = 20;
const SIZE_COLUMNS: usize = 8;

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The fields are drawn by the window, so the window keeps the key focus.
        if !self.focus.is_focused(window) {
            self.focus.focus(window);
        }
        view::prepare(self);

        let body = if self.project.is_none() { self.welcome(cx).into_any_element() } else { self.workspace(cx) };

        div()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|editor, _, _, _| {
                    editor.editing = None;
                }),
            )
            .size_full()
            .flex()
            .flex_col()
            .bg(color(BACKGROUND))
            .text_color(color(TEXT))
            .font_family(fonts::sans())
            .text_sm()
            .child(self.menu_bar(cx))
            .child(div().flex_1().min_h_0().flex().child(body))
            .child(self.status_line())
    }
}

impl Editor {
    fn menu_bar(&self, cx: &mut Context<Self>) -> Div {
        let open = self.project.is_some();
        let (undo, redo) =
            self.project.as_ref().map_or((false, false), |project| (project.can_undo(), project.can_redo()));
        let edited = self.is_entry_edited();

        let project_menu = self.menu(Menu::Project, "Project", cx, |cx| {
            vec![
                action("save", "Save edits", "Ctrl+S", open, cx, Editor::save),
                action("export", "Export rebuilt archives", "Ctrl+E", open, cx, Editor::export),
                separator(false).my(px(GAP_SMALL)).into_any_element(),
                action("close", "Close project", "", true, cx, Editor::close),
            ]
        });
        let edit_menu = self.menu(Menu::Edit, "Edit", cx, |cx| {
            vec![
                action("undo", "Undo", "Ctrl+Z", undo, cx, Editor::undo),
                action("redo", "Redo", "Ctrl+Y", redo, cx, Editor::redo),
                action("revert", "Revert entry to disc", "", edited, cx, Editor::revert_entry),
            ]
        });

        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(GAP_SMALL))
            .px(px(GAP))
            .py(px(GAP_SMALL))
            .bg(color(PANEL))
            .border_b_1()
            .border_color(color(BORDER))
            .child(theme::icon(MENU_ICON).mr(px(GAP_SMALL)))
            .child(project_menu)
            .child(edit_menu)
    }

    /// A menu title, and the open menu under it.
    fn menu(
        &self,
        which: Menu,
        title: &'static str,
        cx: &mut Context<Self>,
        items: impl FnOnce(&mut Context<Self>) -> Vec<AnyElement>,
    ) -> Div {
        let opened = self.menu == Some(which);
        let title = div()
            .id(title)
            .flex()
            .items_center()
            .h(px(CONTROL_HEIGHT))
            .px(px(GAP + GAP_SMALL))
            .rounded(px(theme::CORNER))
            .cursor_pointer()
            .hover(|style| style.bg(color(theme::HOVER)))
            .when(opened, |title| title.bg(theme::selected_fill()))
            .child(title)
            .on_click(cx.listener(move |editor, _, _, cx| {
                editor.menu = if editor.menu == Some(which) { None } else { Some(which) };
                cx.notify();
            }));

        let mut menu = div().flex().flex_col().child(title);
        if opened {
            let panel = menu_panel("menu")
                .on_mouse_down_out(cx.listener(|editor, _, _, cx| {
                    editor.menu = None;
                    cx.notify();
                }))
                .children(items(cx));
            menu = menu.child(deferred(anchored().snap_to_window().child(panel)));
        }
        menu
    }

    fn welcome(&self, cx: &mut Context<Self>) -> Div {
        let path_row = |label: &'static str, which: Field, text: &str, hint: &'static str, cx: &mut Context<Self>| {
            let edit = self.editing.as_ref().filter(|(field, _)| *field == which).map(|(_, edit)| edit);
            let text_owned = text.to_string();
            div()
                .flex()
                .items_center()
                .gap(px(GAP))
                .child(div().w(px(PATH_LABEL_WIDTH)).child(label))
                .child(field(label, text, hint, edit, Some(PATH_WIDTH)).on_click(cx.listener(
                    move |editor, _, _, cx| {
                        editor.start_editing(which, text_owned.clone());
                        cx.notify();
                    },
                )))
                .child(button((label, 1usize), "Browse…", true).on_click(cx.listener(move |editor, _, _, cx| {
                    editor.browse(which, cx);
                })))
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .pt(px(WELCOME_TOP))
            .gap(px(GAP))
            .child(theme::icon(WELCOME_ICON))
            .child(div().text_xl().font_weight(FontWeight::BOLD).child("BOF3 Asset Editor"))
            .child("Browse, view and edit every entry of the game's EMI archives.")
            .child("The disc files are never written: edits and rebuilt archives go to the project folder.")
            .child(div().h(px(WELCOME_GAP)))
            .child(path_row("Disc folder", Field::Disc, &self.disc_field, "the folder that holds BIN/", cx))
            .child(path_row("Project folder", Field::Folder, &self.folder_field, "where edits go", cx))
            .child(button("open", "Open", true).mt(px(GAP)).font_weight(FontWeight::BOLD).on_click(cx.listener(
                |editor, _, _, cx| {
                    editor.open();
                    cx.notify();
                },
            )))
    }

    fn workspace(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut row = div().size_full().flex().child(self.browser(cx));
        if self.archive.is_some() {
            row = row.child(self.entries(cx));
        }
        row.child(self.central(cx)).into_any_element()
    }

    fn browser(&self, cx: &mut Context<Self>) -> Div {
        let edit = self.editing.as_ref().filter(|(field, _)| *field == Field::ArchiveFilter).map(|(_, edit)| edit);
        let filter_text = self.filter.clone();
        let filter = field("archive-filter", &self.filter, "Filter archives", edit, None).on_click(cx.listener(
            move |editor, _, _, cx| {
                editor.start_editing(Field::ArchiveFilter, filter_text.clone());
                cx.notify();
            },
        ));

        let count = self.browser_rows().len();
        let list = uniform_list(
            "archives",
            count,
            cx.processor(|editor, range: std::ops::Range<usize>, _, cx| {
                let rows = editor.browser_rows();
                let selected = editor.archive.as_ref().map(|archive| archive.path.clone());
                range
                    .filter_map(|at| {
                        let row = rows.get(at)?;
                        Some(match row {
                            BrowserRow::Folder { name, open } => {
                                let name = name.clone();
                                let arrow = if *open { "▾" } else { "▸" };
                                selectable(("folder", at), false)
                                    .h(px(ROW_HEIGHT))
                                    .child(format!("{arrow} {name}"))
                                    .on_click(cx.listener(move |editor, _, _, cx| {
                                        editor.toggle_folder(&name);
                                        cx.notify();
                                    }))
                            }
                            BrowserRow::Archive { path, edited } => {
                                let (_, file) = path.split();
                                let marker = if *edited { "●" } else { " " };
                                let path = path.clone();
                                selectable(("archive", at), selected.as_ref() == Some(&path))
                                    .h(px(ROW_HEIGHT))
                                    .pl(px(GAP_SECTION + GAP))
                                    .when(*edited, |row| row.text_color(color(EDITED)))
                                    .child(format!("{marker} {file}"))
                                    .on_click(cx.listener(move |editor, _, _, cx| {
                                        editor.select_archive(&path);
                                        cx.notify();
                                    }))
                            }
                        })
                    })
                    .collect()
            }),
        )
        .track_scroll(self.archive_scroll.clone())
        .flex_1();

        side_panel(ARCHIVES_WIDTH).child(filter).child(list)
    }

    fn entries(&self, cx: &mut Context<Self>) -> Div {
        let Some(archive) = self.archive.as_ref() else { return div() };
        let count = archive.entries().len();

        let list = uniform_list(
            "entries",
            count,
            cx.processor(|editor, range: std::ops::Range<usize>, _, cx| {
                let (Some(project), Some(archive)) = (editor.project.as_ref(), editor.archive.as_ref()) else {
                    return Vec::new();
                };
                range
                    .map(|index| {
                        let toc = &archive.entries()[index];
                        let format = archive.formats[index];
                        let edited = project.is_edited(&(archive.path.clone(), index));
                        let (tag, tag_color) = theme::tag(format);
                        let size = project.payload(archive, index).len();
                        let label =
                            format!("{index:>INDEX_COLUMNS$}  {:<NAME_COLUMNS$} {size:>SIZE_COLUMNS$}", format.name());
                        let text_color = match (edited, format) {
                            (true, _) => color(EDITED),
                            (false, Format::Unclassified) => color(MUTED),
                            (false, _) => color(TEXT),
                        };
                        selectable(("entry", index), editor.entry == Some(index))
                            .h(px(ROW_HEIGHT))
                            .gap(px(GAP))
                            .font_family(fonts::mono())
                            .child(
                                div()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color(tag_color))
                                    .child(format!("{tag:<TAG_COLUMNS$}")),
                            )
                            .child(div().text_color(text_color).whitespace_nowrap().child(label))
                            .tooltip(tip(format!("type {}  load {:#010X}", toc.file_type, toc.ram_ptr)))
                            .on_click(cx.listener(move |editor, _: &ClickEvent, _, cx| {
                                editor.select_entry(index);
                                cx.notify();
                            }))
                    })
                    .collect()
            }),
        )
        .track_scroll(self.entry_scroll.clone())
        .flex_1();

        side_panel(ENTRIES_WIDTH).child(strong(archive.path.0.clone())).child(list)
    }

    fn central(&self, cx: &mut Context<Self>) -> Div {
        let content = match self.current() {
            Some(_) => view::show(self, cx),
            None => muted("Pick an archive, then an entry.").into_any_element(),
        };
        div().flex_1().min_w_0().h_full().flex().flex_col().p(px(GAP)).gap(px(GAP)).child(content)
    }

    fn status_line(&self) -> Div {
        let mut line = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(GAP))
            .h(px(CONTROL_HEIGHT))
            .px(px(GAP))
            .bg(color(PANEL))
            .border_t_1()
            .border_color(color(BORDER));
        if let Some(project) = self.project.as_ref() {
            let dirty = if project.is_dirty() { "unsaved" } else { "saved" };
            line = line.child(mono(format!("{} edits ({dirty})", project.edit_count()))).child(separator(true));
        }
        line.child(div().overflow_hidden().whitespace_nowrap().child(self.status.clone()))
    }
}

/// A list column at the side of the window.
fn side_panel(width: f32) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .w(px(width))
        .h_full()
        .gap(px(GAP_SMALL))
        .p(px(GAP_SMALL))
        .bg(color(PANEL))
        .border_r_1()
        .border_color(color(BORDER))
}

/// A menu line that runs `run` and closes the menu.
fn action(
    id: &'static str,
    label: &str,
    shortcut: &str,
    enabled: bool,
    cx: &mut Context<Editor>,
    run: fn(&mut Editor),
) -> AnyElement {
    let item = menu_item(id, label, shortcut, enabled);
    if !enabled {
        return item.into_any_element();
    }
    item.on_click(cx.listener(move |editor, _, _, cx| {
        editor.menu = None;
        run(editor);
        cx.notify();
    }))
    .into_any_element()
}
