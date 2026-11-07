//! The small controls the window draws itself: buttons, tabs, text fields, menus, check boxes,
//! color swatches, sliders and tooltips. The caller adds the click handlers.

use std::cell::Cell;
use std::ops::RangeInclusive;
use std::rc::Rc;

use gpui::{
    AnyView, App, Bounds, Context, Div, ElementId, FontWeight, IntoElement, MouseButton, MouseDownEvent,
    MouseMoveEvent, Pixels, Render, SharedString, Stateful, Window, canvas, div, prelude::*, px, relative, rgba,
};

use crate::edit::LineEdit;
use crate::fonts;
use crate::theme::{
    self, ACCENT, BACKGROUND, BORDER, CORNER, DISABLED, HOVER, MUTED, PANEL, RAISED, SELECTION, TEXT, color,
};

/// Height of a button, tab or field, in pixels.
pub const CONTROL_HEIGHT: f32 = 26.0;
/// Height of a list row, in pixels.
pub const ROW_HEIGHT: f32 = 22.0;
/// Small, normal and section spacing, in pixels.
pub const GAP_SMALL: f32 = 4.0;
pub const GAP: f32 = 8.0;
pub const GAP_SECTION: f32 = 12.0;
/// Outline widths, in pixels.
pub const OUTLINE_THIN: f32 = 1.0;
pub const OUTLINE_BOLD: f32 = 2.0;

/// Width of a menu or chooser, in pixels.
const MENU_WIDTH: f32 = 280.0;
/// Most height a menu takes before it scrolls, in pixels.
const MENU_HEIGHT: f32 = 360.0;
/// Side of a check box, in pixels.
const CHECK_SIDE: f32 = 14.0;
/// Height of a slider's track and side of its knob, in pixels.
const SLIDER_TRACK: f32 = 6.0;
const SLIDER_KNOB: f32 = 14.0;
/// Width of a slider, in pixels.
const SLIDER_WIDTH: f32 = 180.0;

/// Text in the monospace family.
pub fn mono(text: impl Into<SharedString>) -> Div {
    div().font_family(fonts::mono()).child(text.into())
}

/// Text in the secondary color.
pub fn muted(text: impl Into<SharedString>) -> Div {
    div().text_color(color(MUTED)).child(text.into())
}

/// Bold text.
pub fn strong(text: impl Into<SharedString>) -> Div {
    div().font_weight(FontWeight::BOLD).child(text.into())
}

/// A thin line between two parts of a row or a column.
pub fn separator(vertical: bool) -> Div {
    let line = div().flex_none().bg(color(BORDER));
    if vertical { line.w(px(OUTLINE_THIN)).h(px(CONTROL_HEIGHT * 0.7)) } else { line.h(px(OUTLINE_THIN)).w_full() }
}

/// A button. A disabled button draws dimmed; the caller adds `on_click` only when it is enabled.
pub fn button(id: impl Into<ElementId>, label: impl Into<SharedString>, enabled: bool) -> Stateful<Div> {
    let base = div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .h(px(CONTROL_HEIGHT))
        .px(px(GAP + GAP_SMALL / 2.0))
        .rounded(px(CORNER))
        .bg(color(RAISED))
        .child(label.into());
    if enabled {
        base.cursor_pointer().hover(|style| style.bg(color(HOVER))).active(|style| style.bg(theme::pressed_fill()))
    } else {
        base.text_color(color(DISABLED))
    }
}

/// A row that can be selected: a tab, a list row or a menu choice.
pub fn selectable(id: impl Into<ElementId>, selected: bool) -> Stateful<Div> {
    let base = div().id(id).flex().items_center().px(px(GAP_SMALL + 2.0)).rounded(px(CORNER)).cursor_pointer();
    if selected { base.bg(theme::selected_fill()) } else { base.hover(|style| style.bg(color(HOVER))) }
}

/// A tab of a tab bar.
pub fn tab(id: impl Into<ElementId>, label: impl Into<SharedString>, selected: bool) -> Stateful<Div> {
    let tab = selectable(id, selected).h(px(CONTROL_HEIGHT)).px(px(GAP + GAP_SMALL)).child(label.into());
    if selected { tab.border_b_2().border_color(color(ACCENT)) } else { tab }
}

/// A text field. `edit` is the field's text and cursor while it has the keyboard; otherwise it shows
/// `text`, or `hint` when `text` is empty. The caller adds `on_click` to give it the keyboard.
pub fn field(
    id: impl Into<ElementId>,
    text: &str,
    hint: &str,
    edit: Option<&LineEdit>,
    width: Option<f32>,
) -> Stateful<Div> {
    let (shown, shown_color) = match edit {
        Some(edit) => (edit.shown(), color(TEXT)),
        None if text.is_empty() => (hint.to_string(), color(DISABLED)),
        None => (text.to_string(), color(TEXT)),
    };
    let base = div()
        .id(id)
        .flex()
        .items_center()
        .min_h(px(CONTROL_HEIGHT))
        .px(px(GAP_SMALL + 2.0))
        .rounded(px(CORNER))
        .bg(color(BACKGROUND))
        .border_1()
        .border_color(if edit.is_some() { color(ACCENT) } else { color(BORDER) })
        .text_color(shown_color)
        .overflow_hidden()
        .cursor_text()
        .child(shown);
    match width {
        Some(width) => base.w(px(width)).flex_none().whitespace_nowrap(),
        None => base.w_full(),
    }
}

/// The surface of an open menu or chooser. It closes through the caller's `on_mouse_down_out`.
pub fn menu_panel(id: impl Into<ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_col()
        .min_w(px(MENU_WIDTH))
        .max_h(px(MENU_HEIGHT))
        .overflow_y_scroll()
        .p(px(GAP_SMALL))
        .rounded(px(CORNER))
        .bg(color(PANEL))
        .border_1()
        .border_color(color(HOVER))
        .shadow_lg()
}

/// A line of a menu, with the shortcut on the right.
pub fn menu_item(id: impl Into<ElementId>, label: &str, shortcut: &str, enabled: bool) -> Stateful<Div> {
    let item = div()
        .id(id)
        .flex()
        .items_center()
        .justify_between()
        .gap(px(GAP_SECTION))
        .h(px(CONTROL_HEIGHT))
        .px(px(GAP))
        .rounded(px(CORNER))
        .child(label.to_string())
        .child(muted(shortcut.to_string()));
    if enabled {
        item.cursor_pointer().hover(|style| style.bg(theme::selected_fill()))
    } else {
        item.text_color(color(DISABLED))
    }
}

/// A check box with a label.
pub fn checkbox(id: impl Into<ElementId>, label: &str, checked: bool) -> Stateful<Div> {
    let mut mark = div().flex_none().size(px(CHECK_SIDE)).rounded(px(CORNER / 2.0)).border_1();
    mark = if checked { mark.bg(color(ACCENT)).border_color(color(ACCENT)) } else { mark.border_color(color(MUTED)) };
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(GAP))
        .h(px(CONTROL_HEIGHT))
        .cursor_pointer()
        .child(mark)
        .child(label.to_string())
}

/// A square of one color. `outline` is a width and a theme color.
pub fn swatch(id: impl Into<ElementId>, rgba_bytes: [u8; 4], side: f32, outline: Option<(f32, u32)>) -> Stateful<Div> {
    let [red, green, blue, alpha] = rgba_bytes;
    let fill = rgba(u32::from_be_bytes([red, green, blue, alpha]));
    let base = div().id(id).flex_none().size(px(side)).bg(fill).cursor_pointer();
    match outline {
        Some((width, outline)) => base.border(px(width)).border_color(color(outline)),
        None => base,
    }
}

/// A slider over `range`. `bounds` keeps the track's last painted area, which maps the pointer to a
/// value; `set` receives the new value on press and drag.
pub fn slider<T: 'static>(
    id: impl Into<ElementId>,
    value: u32,
    range: RangeInclusive<u32>,
    bounds: &Rc<Cell<Bounds<Pixels>>>,
    cx: &mut Context<T>,
    set: impl Fn(&mut T, u32) + 'static,
) -> Stateful<Div> {
    let (low, high) = (*range.start(), *range.end());
    let span = high.saturating_sub(low).max(1);
    let fraction = (value.clamp(low, high) - low) as f32 / span as f32;

    let track = Rc::clone(bounds);
    let value_at = Rc::new(move |x: Pixels| {
        let area = track.get();
        if area.size.width <= Pixels::ZERO {
            return low;
        }
        let along = ((x - area.origin.x) / area.size.width).clamp(0.0, 1.0);
        low + (along * span as f32).round() as u32
    });
    let set = Rc::new(set);
    let (pressed_at, pressed_set) = (Rc::clone(&value_at), Rc::clone(&set));
    let measure = Rc::clone(bounds);

    div()
        .id(id)
        .relative()
        .flex_none()
        .w(px(SLIDER_WIDTH))
        .h(px(SLIDER_KNOB))
        .cursor_pointer()
        .child(canvas(move |area, _, _| measure.set(area), |_, (), _, _| {}).absolute().size_full())
        .child(
            div()
                .absolute()
                .top(px((SLIDER_KNOB - SLIDER_TRACK) / 2.0))
                .w_full()
                .h(px(SLIDER_TRACK))
                .rounded_full()
                .bg(color(RAISED))
                .child(div().h_full().w(relative(fraction)).rounded_full().bg(color(ACCENT))),
        )
        .child(
            div()
                .absolute()
                .left(relative(fraction))
                .ml(px(-SLIDER_KNOB / 2.0))
                .size(px(SLIDER_KNOB))
                .rounded_full()
                .bg(color(TEXT)),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                pressed_set(this, pressed_at(event.position.x));
                cx.notify();
            }),
        )
        .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
            if event.dragging() {
                set(this, value_at(event.position.x));
                cx.notify();
            }
        }))
}

/// A tooltip's text.
struct Tip(SharedString);

impl Render for Tip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(GAP))
            .py(px(GAP_SMALL))
            .rounded(px(CORNER))
            .bg(color(PANEL))
            .border_1()
            .border_color(color(HOVER))
            .text_color(color(TEXT))
            .font_family(fonts::mono())
            .text_sm()
            .child(self.0.clone())
    }
}

/// A tooltip builder for `.tooltip(…)` that shows `text`.
pub fn tip(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let text = text.into();
    move |_, cx| cx.new(|_| Tip(text.clone())).into()
}

/// The outline of a selected item, or of an edited one.
pub fn outline(selected: bool, edited: bool) -> Option<(f32, u32)> {
    if selected {
        Some((OUTLINE_BOLD, SELECTION))
    } else if edited {
        Some((OUTLINE_THIN, theme::EDITED))
    } else {
        None
    }
}
