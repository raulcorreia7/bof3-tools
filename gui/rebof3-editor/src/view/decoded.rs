//! The structured reading of a format the decoders know, as a collapsible tree.

use std::collections::HashSet;

use gpui::{AnyElement, Context, Div, div, prelude::*, px};
use rebof3_assets::decode::derive;
use rebof3_formats::Format;
use serde_json::Value;

use super::Entry;
use crate::app::Editor;
use crate::widgets::{GAP, GAP_SMALL, mono, muted, strong};

/// Array items shown before the rest folds into a count.
const ARRAY_PREVIEW: usize = 256;
/// Indent of one tree level, in pixels.
const INDENT: f32 = 16.0;

/// The tree nodes the user opened or closed. An object starts open and an array starts closed.
#[derive(Default)]
pub struct State {
    toggled: HashSet<String>,
}

pub fn decodes(format: Format) -> bool {
    matches!(
        format,
        Format::CueTable
            | Format::AreaActorDefsTable
            | Format::AreaScriptBank
            | Format::BattleScriptBank
            | Format::MagicTable
            | Format::ClutBank
            | Format::Lz77Blob
    )
}

pub fn show(editor: &Editor, entry: &Entry<'_>, cx: &mut Context<Editor>) -> AnyElement {
    // The battle bank at `0x800F0800` and the magic-effect bank at `0x800C3800` have the area bank's
    // format (`wiki: formats/area`).
    let format = match entry.format() {
        Format::BattleScriptBank | Format::MagicTable => Format::AreaScriptBank,
        format => format,
    };
    let derived = match derive(format, entry.load_arg(), entry.payload()) {
        Ok(derived) => derived,
        Err(error) => return muted(format!("The decoder failed: {error}")).into_any_element(),
    };

    let mut column = div().id("decoded").flex().flex_col().flex_1().min_h_0().overflow_scroll();
    for file in &derived {
        match file.role {
            "json" => match serde_json::from_slice::<Value>(&file.data) {
                Ok(value) => column = column.child(tree(&editor.views.decoded, "decoded", "decoded", &value, cx)),
                Err(error) => column = column.child(format!("not JSON: {error}")),
            },
            "decoded" => column = column.child(strong(format!("Decompressed: {} bytes", file.data.len()))),
            _ => {}
        }
    }
    column.into_any_element()
}

fn tree(state: &State, path: &str, name: &str, value: &Value, cx: &mut Context<Editor>) -> Div {
    let (title, open) = match value {
        Value::Object(_) => (name.to_string(), !state.toggled.contains(path)),
        Value::Array(items) => (format!("{name} [{}]", items.len()), state.toggled.contains(path)),
        leaf => return div().flex().gap(px(GAP)).child(strong(name.to_string())).child(mono(leaf.to_string())),
    };

    let key = path.to_string();
    let arrow = if open { "▾" } else { "▸" };
    let header = div()
        .id(gpui::SharedString::from(path.to_string()))
        .cursor_pointer()
        .child(format!("{arrow} {title}"))
        .on_click(cx.listener(move |editor, _, _, cx| {
            let toggled = &mut editor.views.decoded.toggled;
            if !toggled.remove(&key) {
                toggled.insert(key.clone());
            }
            cx.notify();
        }));
    let mut node = div().flex().flex_col().gap(px(GAP_SMALL / 2.0)).child(header);
    if !open {
        return node;
    }

    let mut children = div().flex().flex_col().pl(px(INDENT));
    match value {
        Value::Object(fields) => {
            for (key, field) in fields {
                children = children.child(tree(state, &format!("{path}/{key}"), key, field, cx));
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().take(ARRAY_PREVIEW).enumerate() {
                children = children.child(tree(state, &format!("{path}/{index}"), &index.to_string(), item, cx));
            }
            if items.len() > ARRAY_PREVIEW {
                children = children.child(muted(format!("… {} more", items.len() - ARRAY_PREVIEW)));
            }
        }
        _ => {}
    }
    node = node.child(children);
    node
}
