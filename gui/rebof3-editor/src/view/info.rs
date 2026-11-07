//! What the archive and the classifier know about an entry.

use gpui::{AnyElement, div, prelude::*, px};

use super::Entry;
use crate::theme::{RAISED, READ_ONLY, color};
use crate::widgets::{GAP, GAP_SECTION, GAP_SMALL, mono, strong};

/// Width of the label column, in pixels.
const LABEL_WIDTH: f32 = 140.0;

pub fn show(entry: &Entry<'_>) -> AnyElement {
    let toc = &entry.archive.entries()[entry.index];
    let format = entry.format();
    let original = entry.original().len();
    let current = entry.payload().len();

    let rows = [
        ("Archive", entry.archive.path.0.clone()),
        ("Entry", entry.index.to_string()),
        ("Entry type", toc.file_type.to_string()),
        ("Load argument", format!("{:#010X}", toc.ram_ptr)),
        ("Disc offset", format!("{:#X}", toc.offset)),
        ("Size on disc", format!("{original} bytes")),
        ("Size now", format!("{current} bytes")),
        ("Format", format.name().to_string()),
        ("State", format.state().label().to_string()),
        ("Spec", format.spec_state().to_string()),
        ("Codec", format.codec_state().to_string()),
    ];

    let mut grid = div().flex().flex_col();
    for (at, (label, value)) in rows.into_iter().enumerate() {
        let striped = at % 2 == 1;
        grid = grid.child(
            div()
                .flex()
                .gap(px(GAP))
                .px(px(GAP_SMALL))
                .py(px(GAP_SMALL / 2.0))
                .when(striped, |row| row.bg(color(RAISED)))
                .child(strong(label).w(px(LABEL_WIDTH)).flex_none())
                .child(mono(value)),
        );
    }

    div()
        .id("info")
        .flex()
        .flex_col()
        .flex_1()
        .overflow_y_scroll()
        .child(grid)
        .child(strong("Evidence").mt(px(GAP_SECTION)))
        .child(div().text_color(color(READ_ONLY)).child(format.evidence().to_string()))
        .into_any_element()
}
