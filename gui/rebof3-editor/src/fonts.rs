//! Fonts from the operating system.
//!
//! The editor ships no font files, so its license stays the workspace's. It loads a proportional and a
//! monospace face the system already has; `BOF3_EDIT_FONT` and `BOF3_EDIT_MONO` name other files.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::{Context, FontData, FontDefinitions, FontFamily};

const PROPORTIONAL: &[&str] = &[
    "C:/Windows/Fonts/segoeui.ttf",
    "C:/Windows/Fonts/arial.ttf",
    "/System/Library/Fonts/SFNS.ttf",
    "/System/Library/Fonts/Supplemental/Arial.ttf",
    "/Library/Fonts/Arial.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/liberation/LiberationSans-Regular.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
];

const MONOSPACE: &[&str] = &[
    "C:/Windows/Fonts/consola.ttf",
    "C:/Windows/Fonts/cour.ttf",
    "/System/Library/Fonts/Menlo.ttc",
    "/System/Library/Fonts/Monaco.ttf",
    "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    "/usr/share/fonts/dejavu/DejaVuSansMono.ttf",
    "/usr/share/fonts/noto/NotoSansMono-Regular.ttf",
    "/usr/share/fonts/liberation/LiberationMono-Regular.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
];

fn find(variable: &str, candidates: &[&str], fallback_name: &str) -> Option<Vec<u8>> {
    let named = std::env::var_os(variable).map(PathBuf::from);
    named
        .into_iter()
        .chain(candidates.iter().map(PathBuf::from))
        .find_map(|path| std::fs::read(path).ok())
        .or_else(|| search(Path::new("/usr/share/fonts"), fallback_name))
}

/// The first font file under `folder` whose name starts with `name`.
fn search(folder: &Path, name: &str) -> Option<Vec<u8>> {
    let entries = std::fs::read_dir(folder).ok()?;
    let mut folders = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            folders.push(path);
        } else if path
            .file_name()
            .and_then(|file| file.to_str())
            .is_some_and(|file| file.starts_with(name) && (file.ends_with(".ttf") || file.ends_with(".otf")))
        {
            return std::fs::read(path).ok();
        }
    }
    folders.iter().find_map(|folder| search(folder, name))
}

pub fn install(context: &Context) {
    let mut fonts = FontDefinitions::empty();
    let proportional = find("BOF3_EDIT_FONT", PROPORTIONAL, "DejaVuSans");
    let monospace = find("BOF3_EDIT_MONO", MONOSPACE, "DejaVuSansMono").or_else(|| proportional.clone());
    let proportional = proportional.or_else(|| monospace.clone());
    let (Some(proportional), Some(monospace)) = (proportional, monospace) else {
        eprintln!("bof3-edit: no system font found; set BOF3_EDIT_FONT to a .ttf file");
        std::process::exit(2);
    };
    fonts.font_data.insert("sans".into(), Arc::new(FontData::from_owned(proportional)));
    fonts.font_data.insert("mono".into(), Arc::new(FontData::from_owned(monospace)));
    fonts.families.insert(FontFamily::Proportional, vec!["sans".into(), "mono".into()]);
    fonts.families.insert(FontFamily::Monospace, vec!["mono".into(), "sans".into()]);
    context.set_fonts(fonts);
}
