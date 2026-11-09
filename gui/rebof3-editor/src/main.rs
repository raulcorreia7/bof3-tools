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
mod edit;
mod fonts;
mod theme;
mod view;
mod widgets;
mod window;

/// The retail-input helper the integration tests also use.
#[cfg(test)]
#[path = "../tests/support/mod.rs"]
mod retail_support;

use std::cell::Cell;
use std::path::PathBuf;
use std::process::ExitCode;
use std::rc::Rc;

use gpui::{App, AppContext, Application, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size};

pub use rebof3_editor::{catalog, project, schema};

/// The window's title and the application id desktops group it by.
const TITLE: &str = "BOF3 Asset Editor";
const APP_ID: &str = "bof3-edit";
/// The window's first size and its smallest size, in pixels.
const WINDOW_SIZE: (f32, f32) = (1400.0, 860.0);
const WINDOW_MIN_SIZE: (f32, f32) = (800.0, 500.0);

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1).map(PathBuf::from);
    let disc = args.next();
    let folder = args.next();
    let failed = Rc::new(Cell::new(false));
    let opened = Rc::clone(&failed);
    let Some(application) = start() else { return ExitCode::FAILURE };

    application.run(move |cx: &mut App| {
        fonts::install(cx);
        let bounds = Bounds::centered(None, size(px(WINDOW_SIZE.0), px(WINDOW_SIZE.1)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(WINDOW_MIN_SIZE.0), px(WINDOW_MIN_SIZE.1))),
            titlebar: Some(TitlebarOptions { title: Some(TITLE.into()), ..Default::default() }),
            app_id: Some(APP_ID.into()),
            ..Default::default()
        };
        let window = cx.open_window(options, |window, cx| {
            let editor = cx.new(|cx| app::Editor::new(disc, folder, cx));
            editor.read(cx).focus.focus(window);
            let closing = editor.clone();
            window.on_window_should_close(cx, move |window, cx| {
                closing.update(cx, |editor, cx| editor.should_close(window, cx))
            });
            editor
        });
        match window {
            Ok(_) => cx.activate(true),
            Err(error) => {
                eprintln!("bof3-edit: cannot open a window: {error}");
                opened.set(true);
                cx.quit();
            }
        }
    });
    if failed.get() { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}

/// Starts the toolkit, or explains why it cannot start. gpui panics when the system gives it no GPU
/// context (it has no software renderer), so the panic is caught and reported as an error.
fn start() -> Option<Application> {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let started = std::panic::catch_unwind(Application::new);
    std::panic::set_hook(hook);
    started
        .map_err(|panic| {
            let reason = panic
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| panic.downcast_ref::<&str>().copied())
                .unwrap_or("unknown error");
            eprintln!(
                "bof3-edit: cannot start the window: {reason}.\nCheck that a Vulkan graphics driver is installed and the display is reachable."
            );
        })
        .ok()
}
