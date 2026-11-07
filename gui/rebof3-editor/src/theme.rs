//! The editor's look: a dark theme with one warm accent, its window icon, and the short colored tag
//! each entry format wears in the lists.

use eframe::egui::{self, Color32, CornerRadius, IconData, Stroke, Visuals};
use rebof3_emi::classify::Format;

/// The accent: selection, focus and the active tab.
pub const ACCENT: Color32 = Color32::from_rgb(232, 142, 66);

const BACKGROUND: Color32 = Color32::from_rgb(22, 24, 30);
const PANEL: Color32 = Color32::from_rgb(28, 31, 38);
const RAISED: Color32 = Color32::from_rgb(38, 42, 52);
const HOVER: Color32 = Color32::from_rgb(50, 55, 68);
const TEXT: Color32 = Color32::from_rgb(222, 224, 230);
const CORNER: u8 = 6;
/// How strongly the accent tints a selection and a pressed widget.
const SELECTION_TINT: f32 = 0.45;
const PRESSED_TINT: f32 = 0.6;
/// Outline of a selection, in points.
const SELECTION_OUTLINE: f32 = 1.0;
/// Spacing between widgets, inside buttons, and the height of a control row, in points.
const ITEM_SPACING: [f32; 2] = [8.0, 6.0];
const BUTTON_PADDING: [f32; 2] = [10.0, 4.0];
const CONTROL_HEIGHT: f32 = 24.0;
/// Gap between two cells of the icon's grid, in pixels.
const ICON_GAP: usize = 2;
const OPAQUE: u8 = 255;

pub fn apply(context: &egui::Context) {
    let mut visuals = Visuals::dark();
    visuals.panel_fill = PANEL;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = BACKGROUND;
    visuals.faint_bg_color = RAISED;
    visuals.override_text_color = Some(TEXT);
    visuals.selection.bg_fill = ACCENT.linear_multiply(SELECTION_TINT);
    visuals.selection.stroke = Stroke::new(SELECTION_OUTLINE, ACCENT);
    visuals.hyperlink_color = ACCENT;

    let corner = CornerRadius::same(CORNER);
    for widget in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = corner;
    }
    visuals.widgets.inactive.weak_bg_fill = RAISED;
    visuals.widgets.hovered.weak_bg_fill = HOVER;
    visuals.widgets.active.weak_bg_fill = ACCENT.linear_multiply(PRESSED_TINT);
    visuals.window_corner_radius = corner;
    context.set_visuals(visuals);

    context.global_style_mut(|style| {
        style.spacing.item_spacing = ITEM_SPACING.into();
        style.spacing.button_padding = BUTTON_PADDING.into();
        style.spacing.interact_size.y = CONTROL_HEIGHT;
    });
}

/// A short tag and color for an entry format.
pub fn tag(format: Format) -> (&'static str, Color32) {
    match format {
        Format::VramUpload => ("TEX", Color32::from_rgb(90, 170, 240)),
        Format::ClutBank => ("PAL", Color32::from_rgb(210, 110, 200)),
        Format::VabHeader | Format::VabBody => ("VAB", Color32::from_rgb(110, 200, 130)),
        Format::SepPackage => ("SEQ", Color32::from_rgb(80, 200, 180)),
        Format::CueTable => ("CUE", Color32::from_rgb(150, 210, 90)),
        Format::AreaMessageBank => ("TXT", Color32::from_rgb(240, 200, 90)),
        Format::Lz77Blob => ("LZ", Color32::from_rgb(170, 150, 230)),
        Format::CharacterBlob => ("CHR", Color32::from_rgb(240, 120, 110)),
        Format::AreaCellGrid | Format::AreaMapBundle | Format::AreaAuxWindow => {
            ("MAP", Color32::from_rgb(120, 180, 110))
        }
        Format::AreaScriptBank | Format::BattleScriptBank => ("SCR", Color32::from_rgb(230, 160, 90)),
        Format::AreaActorDefsTable | Format::AreaEncounterTable | Format::SharedResourceTable | Format::MagicTable => {
            ("TAB", Color32::from_rgb(200, 190, 120))
        }
        Format::CodeImage => ("EXE", Color32::from_rgb(150, 155, 170)),
        Format::Reserve => ("RES", Color32::from_rgb(110, 115, 130)),
        Format::Unclassified => ("???", Color32::from_rgb(110, 115, 130)),
    }
}

/// Side of the window icon, in pixels.
const ICON_SIDE: usize = 64;
/// Cells across the icon's palette grid.
const ICON_CELLS: usize = 4;
/// Margin around the grid, in pixels.
const ICON_MARGIN: usize = 8;

/// The window icon: a 4×4 palette grid, the editor's subject, on a rounded dark tile.
pub fn icon() -> IconData {
    let colors = [
        [232, 142, 66],
        [240, 200, 90],
        [210, 110, 200],
        [90, 170, 240],
        [110, 200, 130],
        [240, 120, 110],
        [80, 200, 180],
        [170, 150, 230],
        [150, 210, 90],
        [230, 160, 90],
        [120, 180, 110],
        [200, 190, 120],
        [90, 120, 200],
        [240, 160, 160],
        [160, 220, 240],
        [222, 224, 230],
    ];
    let cell = (ICON_SIDE - 2 * ICON_MARGIN) / ICON_CELLS;
    let mut rgba = Vec::with_capacity(ICON_SIDE * ICON_SIDE * 4);

    for y in 0..ICON_SIDE {
        for x in 0..ICON_SIDE {
            let inside = rounded(x, y, ICON_SIDE, ICON_MARGIN);
            let pixel = match grid_cell(x, y, cell, ICON_GAP) {
                Some(index) => [colors[index][0], colors[index][1], colors[index][2], OPAQUE],
                None if inside => [BACKGROUND.r(), BACKGROUND.g(), BACKGROUND.b(), OPAQUE],
                None => [0, 0, 0, 0],
            };
            rgba.extend_from_slice(&pixel);
        }
    }
    IconData { rgba, width: ICON_SIDE as u32, height: ICON_SIDE as u32 }
}

/// The palette cell under pixel (`x`, `y`), or `None` in a margin or gap.
fn grid_cell(x: usize, y: usize, cell: usize, gap: usize) -> Option<usize> {
    let (gx, gy) = (x.checked_sub(ICON_MARGIN)?, y.checked_sub(ICON_MARGIN)?);
    let (column, row) = (gx / cell, gy / cell);
    let inner = gx % cell >= gap && gy % cell >= gap;
    (column < ICON_CELLS && row < ICON_CELLS && inner).then_some(row * ICON_CELLS + column)
}

/// True inside a square of `side` pixels with corners rounded by `radius`.
fn rounded(x: usize, y: usize, side: usize, radius: usize) -> bool {
    let clamp = |value: usize| value.clamp(radius, side - 1 - radius);
    let (dx, dy) = (x.abs_diff(clamp(x)), y.abs_diff(clamp(y)));
    dx * dx + dy * dy <= radius * radius
}
