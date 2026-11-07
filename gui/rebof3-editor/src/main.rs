//! `bof3-edit`: the asset editor for the EMI archives of Breath of Fire III.
//!
//! ```text
//! bof3-edit [DISC_FOLDER [PROJECT_FOLDER]]
//! ```
//!
//! `DISC_FOLDER` is an extracted disc (the folder that holds `BIN/`); it is only read. Edits and
//! rebuilt archives go to `PROJECT_FOLDER` (default: `bof3-project` next to the disc folder).

#![forbid(unsafe_code)]

mod app;
mod audio;
mod fonts;
mod theme;
mod view;

use std::path::PathBuf;
use std::sync::Arc;

pub use rebof3_editor::{project, schema};

/// The window's first size and its smallest size, in points.
const WINDOW_SIZE: [f32; 2] = [1400.0, 860.0];
const WINDOW_MIN_SIZE: [f32; 2] = [800.0, 500.0];

fn main() -> eframe::Result<()> {
    let mut args = std::env::args_os().skip(1).map(PathBuf::from);
    let disc = args.next();
    let folder = args.next();
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("BOF3 Asset Editor")
            .with_app_id("bof3-edit")
            .with_icon(Arc::new(theme::icon()))
            .with_inner_size(WINDOW_SIZE)
            .with_min_inner_size(WINDOW_MIN_SIZE),
        ..Default::default()
    };
    eframe::run_native(
        "BOF3 Asset Editor",
        options,
        Box::new(move |cc| {
            fonts::install(&cc.egui_ctx);
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(app::App::new(disc, folder)))
        }),
    )
}
