//! A texture upload as a paintable image.
//!
//! The upload's VRAM words read at 4, 8 or 16 bits per texel. A 4- or 8-bit texture is drawn with a
//! palette row: one the draw sites join to it (`rebof3_assets::decode::joined_rows`), a row of a palette
//! bank in the same archive, or a grey ramp. Painting writes texel indices (or 16-bit colors) back into
//! the words and the words back into the upload's rectangles.

use eframe::egui::{self, Color32, RichText, TextureHandle, TextureOptions, Vec2};
use psx_formats::gfx::texture::{Depth, texel_indices, texel_words};
use psx_formats::gfx::vram;
use rebof3_assets::decode::{RowUse, color_rgba, joined_rows, texture_words};
use rebof3_emi::classify::Format;

use super::{Entry, style};

const ZOOM_MIN: f32 = 1.0;
const ZOOM_MAX: f32 = 16.0;
const ZOOM_DEFAULT: f32 = 2.0;
const ZOOM_STEP: f64 = 1.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Bits {
    Four,
    Eight,
    Sixteen,
}

impl Bits {
    fn depth(self) -> Option<Depth> {
        match self {
            Bits::Four => Some(Depth::Four),
            Bits::Eight => Some(Depth::Eight),
            Bits::Sixteen => None,
        }
    }
}

/// Where the colors of a 4- or 8-bit texture come from.
#[derive(Clone, Debug, PartialEq)]
enum Source {
    /// A palette row a draw site joins to the texture.
    Joined(usize),
    /// Row `row` of the palette bank in entry `bank`.
    Bank {
        bank: usize,
        row: usize,
    },
    Grey,
}

pub struct State {
    key: Option<(String, usize)>,
    bits: Bits,
    source: Source,
    zoom: f32,
    /// The palette index (4/8 bit) or color word (16 bit) the pencil paints.
    ink: u16,
    joined: Vec<RowUse>,
    image: Option<TextureHandle>,
    drawn: Vec<u8>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            key: None,
            bits: Bits::Four,
            source: Source::Grey,
            zoom: ZOOM_DEFAULT,
            ink: 0,
            joined: Vec::new(),
            image: None,
            drawn: Vec::new(),
        }
    }
}

/// The colors a texture is drawn with.
fn palette(entry: &Entry<'_>, state: &State, depth: Depth) -> Vec<u16> {
    let colors = depth.colors();
    match &state.source {
        Source::Joined(at) => state.joined.get(*at).map(|row| row.words.clone()).unwrap_or_default(),
        Source::Bank { bank, row } => {
            let words = words_of(entry.sibling(*bank));
            words.chunks(colors).nth(*row).map(<[u16]>::to_vec).unwrap_or_default()
        }
        Source::Grey => (0..colors).map(|index| grey(index, colors)).collect(),
    }
}

/// A 15-bit grey word for palette index `index` of `colors`.
fn grey(index: usize, colors: usize) -> u16 {
    const CHANNEL_MAX: usize = 31;
    let level = (index * CHANNEL_MAX / (colors - 1)) as u16;
    level | level << 5 | level << 10
}

fn words_of(bytes: &[u8]) -> Vec<u16> {
    bytes.as_chunks::<2>().0.iter().map(|pair| u16::from_le_bytes(*pair)).collect()
}

/// Picks the depth and colors the first time an entry is shown: a joined row when there is one.
fn reset(entry: &Entry<'_>, state: &mut State) {
    let payloads: Vec<&[u8]> = (0..entry.archive.entries().len()).map(|index| entry.sibling(index)).collect();
    let rows = joined_rows(&entry.archive.path.0, entry.archive.entries(), &payloads);
    state.joined = rows.get(&entry.index).cloned().unwrap_or_default();
    state.zoom = ZOOM_DEFAULT;
    state.ink = 0;
    match state.joined.first() {
        Some(row) => {
            state.bits = if row.depth == Depth::Four { Bits::Four } else { Bits::Eight };
            state.source = Source::Joined(0);
        }
        None => {
            state.bits = Bits::Sixteen;
            state.source = Source::Grey;
        }
    }
}

pub fn show(ui: &mut egui::Ui, entry: &mut Entry<'_>, state: &mut State) -> Option<String> {
    let key = (entry.archive.path.0.clone(), entry.index);
    if state.key.as_ref() != Some(&key) {
        reset(entry, state);
        state.key = Some(key);
        state.image = None;
    }

    let words = match texture_words(entry.load_arg(), entry.payload()) {
        Ok(words) => words,
        Err(error) => return Some(format!("The upload does not decode: {error}")),
    };
    let round_trips =
        vram::payload_from_grid(words.width, words.height, &words.words).is_ok_and(|bytes| bytes == entry.payload());

    controls(ui, entry, state, &words);
    ui.separator();

    let texels_per_word = state.bits.depth().map_or(1, Depth::texels_per_word);
    let width = words.width * texels_per_word;
    let height = words.height;
    let (pixels, values) = render(entry, state, &words.words);

    if state.image.is_none() || state.drawn != pixels {
        let image = egui::ColorImage::from_rgba_unmultiplied([width, height], &pixels);
        state.image = Some(ui.ctx().load_texture("texture", image, TextureOptions::NEAREST));
        state.drawn = pixels;
    }
    let image = state.image.clone()?;

    let mut message = None;
    egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
        let size = Vec2::new(width as f32, height as f32) * state.zoom;
        let response = ui.add(egui::Image::new(&image).fit_to_exact_size(size).sense(egui::Sense::click_and_drag()));
        let Some(pointer) = response.hover_pos() else { return };

        let local = (pointer - response.rect.min) / state.zoom;
        let (x, y) = (local.x as usize, local.y as usize);
        if x >= width || y >= height {
            return;
        }
        let value = values[y * width + x];
        response.clone().on_hover_text(format!("x {x}  y {y}  value {value:#X}"));

        if response.dragged_by(egui::PointerButton::Primary) || response.clicked() {
            if !round_trips {
                message = Some("This upload does not round-trip through the rectangle writer; painting is off.".into());
            } else if let Some(bytes) = paint(state, &words.words, words.width, words.height, x, y) {
                entry.set(bytes);
            }
        }
        if response.clicked_by(egui::PointerButton::Secondary) {
            state.ink = value;
        }
    });
    message
}

/// The RGBA pixels of the texture, and the raw value (index or word) under each pixel.
fn render(entry: &Entry<'_>, state: &State, words: &[u16]) -> (Vec<u8>, Vec<u16>) {
    match state.bits.depth() {
        None => {
            let pixels = words.iter().flat_map(|&word| color_rgba(word)).collect();
            (pixels, words.to_vec())
        }
        Some(depth) => {
            let colors = palette(entry, state, depth);
            let indices = texel_indices(words, depth);
            let pixels = indices
                .iter()
                .flat_map(|&index| colors.get(usize::from(index)).map_or([0, 0, 0, 0], |&word| color_rgba(word)))
                .collect();
            (pixels, indices.iter().map(|&index| u16::from(index)).collect())
        }
    }
}

/// Writes the ink at texel (`x`, `y`) and returns the new payload, or `None` when nothing changed.
fn paint(state: &State, words: &[u16], width: usize, height: usize, x: usize, y: usize) -> Option<Vec<u8>> {
    let mut words = words.to_vec();
    match state.bits.depth() {
        None => {
            let at = y * width + x;
            if words[at] == state.ink {
                return None;
            }
            words[at] = state.ink;
        }
        Some(depth) => {
            let mut indices = texel_indices(&words, depth);
            let at = y * width * depth.texels_per_word() + x;
            let ink = state.ink.min(depth.colors() as u16 - 1) as u8;
            if indices[at] == ink {
                return None;
            }
            indices[at] = ink;
            words = texel_words(&indices, depth);
        }
    }
    vram::payload_from_grid(width, height, &words).ok()
}

fn controls(ui: &mut egui::Ui, entry: &Entry<'_>, state: &mut State, words: &rebof3_assets::decode::TextureWords) {
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new(format!("VRAM x {} y {}  {}×{} words", words.x, words.y, words.width, words.height))
                .monospace(),
        );
        ui.separator();
        ui.label("Depth");
        ui.selectable_value(&mut state.bits, Bits::Four, "4-bit");
        ui.selectable_value(&mut state.bits, Bits::Eight, "8-bit");
        ui.selectable_value(&mut state.bits, Bits::Sixteen, "16-bit");
        ui.separator();
        ui.label("Zoom");
        ui.add(egui::Slider::new(&mut state.zoom, ZOOM_MIN..=ZOOM_MAX).step_by(ZOOM_STEP));
    });

    let Some(depth) = state.bits.depth() else {
        ui.label(RichText::new(format!("Ink: color word {:#06X} (right-click a texel to pick)", state.ink)).weak());
        return;
    };

    ui.horizontal_wrapped(|ui| {
        ui.label("Colors");
        let current = describe(&state.source, &state.joined);
        egui::ComboBox::from_id_salt("palette").selected_text(current).width(style::CHOOSER_WIDTH).show_ui(ui, |ui| {
            for (at, row) in state.joined.iter().enumerate() {
                let label = format!("joined: bank {} clut {},{}", row.palette, row.clut.x, row.clut.y);
                ui.selectable_value(&mut state.source, Source::Joined(at), label).on_hover_text(&row.evidence);
            }
            for (index, format) in entry.archive.formats.iter().enumerate() {
                if *format != Format::ClutBank {
                    continue;
                }
                let rows = entry.sibling(index).len() / 2 / depth.colors();
                for row in 0..rows {
                    ui.selectable_value(
                        &mut state.source,
                        Source::Bank { bank: index, row },
                        format!("bank {index} row {row}"),
                    );
                }
            }
            ui.selectable_value(&mut state.source, Source::Grey, "grey ramp");
        });
    });

    let colors = palette(entry, state, depth);
    ui.horizontal_wrapped(|ui| {
        ui.label("Ink");
        ui.spacing_mut().item_spacing.x = style::SWATCH_GAP;
        for (index, &word) in colors.iter().enumerate() {
            let [red, green, blue, alpha] = color_rgba(word);
            let (rect, response) = ui.allocate_exact_size(Vec2::splat(style::SWATCH), egui::Sense::click());
            ui.painter().rect_filled(rect, style::SQUARE, Color32::from_rgba_unmultiplied(red, green, blue, alpha));
            if usize::from(state.ink) == index {
                ui.painter().rect_stroke(
                    rect,
                    style::SQUARE,
                    (style::OUTLINE_BOLD, style::SELECTION),
                    egui::StrokeKind::Inside,
                );
            }
            if response.on_hover_text(format!("index {index}  word {word:#06X}")).clicked() {
                state.ink = index as u16;
            }
        }
    });
}

fn describe(source: &Source, joined: &[RowUse]) -> String {
    match source {
        Source::Joined(at) => joined.get(*at).map_or("joined row".into(), |row| {
            format!("joined: bank {} clut {},{}", row.palette, row.clut.x, row.clut.y)
        }),
        Source::Bank { bank, row } => format!("bank {bank} row {row}"),
        Source::Grey => "grey ramp".into(),
    }
}
