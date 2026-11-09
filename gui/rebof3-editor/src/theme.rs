//! The editor's look: a dark theme with one warm accent, its icon, and the short colored tag each
//! entry format wears in the lists. Each color is a 24-bit `0xRRGGBB` value.

use gpui::{Div, Hsla, div, prelude::*, px, rgb};
use rebof3_formats::Format;

/// The accent: selection, focus and the active tab.
pub const ACCENT: u32 = 0xe88e42;

/// The window backdrop and the inside of text fields.
pub const BACKGROUND: u32 = 0x16181e;
/// Panels and lists.
pub const PANEL: u32 = 0x1c1f26;
/// Buttons, tabs and the tracks of sliders.
pub const RAISED: u32 = 0x262a34;
/// The fill under the pointer.
pub const HOVER: u32 = 0x323744;
/// Panel borders and dividers.
pub const BORDER: u32 = 0x2c303b;
/// Body text.
pub const TEXT: u32 = 0xdee0e6;
/// Secondary text: hints, offsets and unclassified entries.
pub const MUTED: u32 = 0x8a8f9c;
/// Controls that cannot be used.
pub const DISABLED: u32 = 0x5a5f6c;

/// An edited entry, byte or cell.
pub const EDITED: u32 = 0xe6aa3c;
/// A value the view cannot write back.
pub const READ_ONLY: u32 = 0x9696a0;
/// The outline of the selected texel, byte or color.
pub const SELECTION: u32 = 0xffe650;
/// Text drawn on a [`SELECTION`] fill.
pub const ON_SELECTION: u32 = 0x000000;

/// How strongly the accent tints a selected row and a pressed control.
const SELECTION_TINT: f32 = 0.45;
const PRESSED_TINT: f32 = 0.6;

/// Corner radius of controls, rows and fields, in pixels.
pub const CORNER: f32 = 6.0;

/// A theme color as a gpui color.
pub fn color(value: u32) -> Hsla {
    rgb(value).into()
}

/// The fill of a selected row or tab: the accent, see-through.
pub fn selected_fill() -> Hsla {
    Hsla { a: SELECTION_TINT, ..color(ACCENT) }
}

/// The fill of a pressed control.
pub fn pressed_fill() -> Hsla {
    Hsla { a: PRESSED_TINT, ..color(ACCENT) }
}

/// A short tag and color for an entry format.
pub fn tag(format: Format) -> (&'static str, u32) {
    match format {
        Format::VramUpload => ("TEX", 0x5aaaf0),
        Format::ClutBank => ("PAL", 0xd26ec8),
        Format::VabHeader | Format::VabBody => ("VAB", 0x6ec882),
        Format::SepPackage => ("SEQ", 0x50c8b4),
        Format::CueTable => ("CUE", 0x96d25a),
        Format::AreaMessageBank => ("TXT", 0xf0c85a),
        Format::Lz77Blob => ("LZ", 0xaa96e6),
        Format::CharacterBlob => ("CHR", 0xf0786e),
        Format::AreaCellGrid | Format::AreaMapBundle | Format::AreaAuxWindow => ("MAP", 0x78b46e),
        Format::AreaScriptBank | Format::BattleScriptBank | Format::MagicTable => ("SCR", 0xe6a05a),
        Format::AreaActorDefsTable | Format::AreaEncounterTable | Format::SharedResourceTable => ("TAB", 0xc8be78),
        Format::CodeImage => ("EXE", 0x969baa),
        Format::Reserve => ("RES", 0x6e7382),
        Format::Unclassified => ("???", 0x6e7382),
    }
}

/// Cells across the icon's palette grid.
const ICON_CELLS: usize = 4;
/// The colors of the icon's grid, row by row.
const ICON_COLORS: [u32; ICON_CELLS * ICON_CELLS] = [
    0xe88e42, 0xf0c85a, 0xd26ec8, 0x5aaaf0, //
    0x6ec882, 0xf0786e, 0x50c8b4, 0xaa96e6, //
    0x96d25a, 0xe6a05a, 0x78b46e, 0xc8be78, //
    0x5a78c8, 0xf0a0a0, 0xa0dcf0, 0xdee0e6,
];
/// The icon's margin and the gap between two cells, as fractions of its side.
const ICON_MARGIN: f32 = 0.125;
const ICON_GAP: f32 = 0.03;
/// The icon tile's corner radius, as a fraction of its side.
const ICON_CORNER: f32 = 0.125;

/// The editor's icon: a 4×4 palette grid, the editor's subject, on a rounded dark tile. gpui sets
/// no window icon, so the window draws it in its menu bar and on the welcome screen.
pub fn icon(side: f32) -> Div {
    let margin = side * ICON_MARGIN;
    let gap = side * ICON_GAP;
    let mut grid = div().flex().flex_col().gap(px(gap));

    for row in ICON_COLORS.chunks(ICON_CELLS) {
        let mut cells = div().flex().flex_1().gap(px(gap));
        for &cell in row {
            cells = cells.child(div().flex_1().bg(color(cell)));
        }
        grid = grid.child(cells);
    }

    div()
        .flex()
        .flex_none()
        .size(px(side))
        .p(px(margin))
        .rounded(px(side * ICON_CORNER))
        .bg(color(BACKGROUND))
        .child(grid.size_full())
}
