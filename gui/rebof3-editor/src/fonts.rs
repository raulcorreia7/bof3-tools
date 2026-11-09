//! Fonts from the operating system.
//!
//! The editor ships no font files, so its license stays the workspace's. It uses the first
//! proportional and monospace family the system has; `BOF3_EDIT_FONT` and `BOF3_EDIT_MONO` name other
//! families.

use std::sync::OnceLock;

use gpui::{App, SharedString};

const PROPORTIONAL: &[&str] =
    &["Segoe UI", "SF Pro Text", "Helvetica Neue", "Inter", "Noto Sans", "DejaVu Sans", "Liberation Sans", "Arial"];

const MONOSPACE: &[&str] = &[
    "Consolas",
    "Menlo",
    "SF Mono",
    "JetBrains Mono",
    "DejaVu Sans Mono",
    "Noto Sans Mono",
    "Liberation Mono",
    "Courier New",
];

/// gpui's name for the platform's interface font, used when no listed family is installed.
const SYSTEM_FONT: &str = ".SystemUIFont";

/// The two font families the window draws with.
struct Fonts {
    sans: SharedString,
    mono: SharedString,
}

static FONTS: OnceLock<Fonts> = OnceLock::new();

/// The proportional family.
pub fn sans() -> SharedString {
    FONTS.get().map_or_else(|| SYSTEM_FONT.into(), |fonts| fonts.sans.clone())
}

/// The monospace family, for offsets, bytes and values.
pub fn mono() -> SharedString {
    FONTS.get().map_or_else(|| SYSTEM_FONT.into(), |fonts| fonts.mono.clone())
}

/// Picks the families once: the named one when the system has it, else the first listed one it has.
pub fn install(cx: &App) {
    FONTS.get_or_init(|| pick(cx));
}

fn pick(cx: &App) -> Fonts {
    let installed = cx.text_system().all_font_names();
    let has = |name: &str| installed.iter().any(|family| family == name);
    let choose = |variable: &str, candidates: &[&str]| {
        let named = std::env::var(variable).ok().filter(|name| has(name));
        named.or_else(|| candidates.iter().find(|name| has(name)).map(|name| (*name).to_string()))
    };

    let sans = choose("BOF3_EDIT_FONT", PROPORTIONAL);
    let mono = choose("BOF3_EDIT_MONO", MONOSPACE).or_else(|| sans.clone());
    let sans = sans.unwrap_or_else(|| SYSTEM_FONT.to_string());
    let mono = mono.unwrap_or_else(|| sans.clone());
    Fonts { sans: sans.into(), mono: mono.into() }
}
