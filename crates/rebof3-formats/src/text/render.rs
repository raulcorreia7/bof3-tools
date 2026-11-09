//! Draws a laid-out page the way the GPU would.
//! semi-transparency (psx-spx, "GPU Rendering Attributes" and "GPU Render Polygon Commands").
//! `B + F` where STP is set (`func_8014FB80 (exe/slus_004_22@0x8014FB80)`, texture page `0x2F`).

use super::font::{CELL_HEIGHT, CELL_WIDTH, Font, TILE_SIDE, TILE_TOP, message_cell, number_cell};
use super::layout::{Choice, GLYPH_ADVANCE, Glyph, Page, Shape};

/// The frame's outer margin, in pixels (the outer line sits two pixels outside the window).
pub const MARGIN: i32 = 2;
/// The STP bit of a colour word.
const STP: u16 = 0x8000;
/// The vertex colour that leaves a texel unchanged, and the frame's.
const NEUTRAL: u32 = 128;
const FRAME_COLOR: u32 = 255;
/// Frame line greys (`func_80159B9C`).
const OUTLINE_GREY: u8 = 200;
const OUTER_GREY: u8 = 0x8C;

/// An RGBA image.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Rgba {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}

struct Canvas {
    width: i32,
    height: i32,
    words: Vec<[u8; 3]>,
}

type Channels = [u32; 3];

fn channels(word: u16) -> Channels {
    [u32::from(word & 31), u32::from((word >> 5) & 31), u32::from((word >> 10) & 31)]
}

impl Canvas {
    fn new(width: i32, height: i32, backdrop: [u8; 3]) -> Self {
        Self { width, height, words: vec![backdrop.map(|c| c >> 3); (width * height) as usize] }
    }

    fn put(&mut self, x: i32, y: i32, back: impl Fn(Channels) -> Channels) {
        if (0..self.width).contains(&x) && (0..self.height).contains(&y) {
            let at = (y * self.width + x) as usize;
            let old = self.words[at].map(u32::from);
            self.words[at] = back(old).map(|c| c.min(31) as u8);
        }
    }

    /// A textured pixel: modulated by `color / 128`, then blended when the word sets STP and `blend` is given.
    fn texel(&mut self, x: i32, y: i32, word: u16, color: u32, blend: Blend) {
        if word == 0 {
            return;
        }
        let front = channels(word).map(|c| (c * color / NEUTRAL).min(31));
        let semi = word & STP != 0;
        self.put(x, y, |back| match (semi, blend) {
            (true, Blend::Average) => [0, 1, 2].map(|i| (back[i] + front[i]) / 2),
            (true, Blend::Add) => [0, 1, 2].map(|i| back[i] + front[i]),
            _ => front,
        });
    }

    fn rgba(&self) -> Rgba {
        let pixels = self
            .words
            .iter()
            .flat_map(|c| [c[0] << 3 | c[0] >> 2, c[1] << 3 | c[1] >> 2, c[2] << 3 | c[2] >> 2, 255])
            .collect();
        Rgba { width: self.width as usize, height: self.height as usize, pixels }
    }
}

/// The two blend modes the message window uses (`abr` 0 and 1).
#[derive(Clone, Copy)]
enum Blend {
    Average,
    Add,
}

/// Draws the first `glyphs` glyphs of `page` in a window of `size`, over `backdrop`. `framed` draws the frame.
pub fn window(font: &Font, page: &Page, size: (i32, i32), framed: bool, glyphs: usize, backdrop: [u8; 3]) -> Rgba {
    let mut canvas = Canvas::new(size.0 + 2 * MARGIN, size.1 + 2 * MARGIN, backdrop);
    if framed {
        frame(font, &mut canvas, size);
    }
    for glyph in page.glyphs.iter().take(glyphs) {
        draw(font, &mut canvas, glyph, (MARGIN, MARGIN));
    }
    canvas.rgba()
}

/// Draws the choice panel's answers over `backdrop`, at the panel's size.
pub fn choice(font: &Font, choice: &Choice, backdrop: [u8; 3]) -> Rgba {
    let (_, top, width, height) = choice.panel();
    let mut canvas = Canvas::new(width + 2 * MARGIN, height + 2 * MARGIN, backdrop);
    frame(font, &mut canvas, (width, height));
    for (row, answer) in choice.answers.iter().enumerate() {
        let (x, y) = choice.answer_origin(row);
        let offset = (MARGIN + x - super::layout::CHOICE_PANEL_X, MARGIN + y - top);
        for glyph in &answer.glyphs {
            draw(font, &mut canvas, glyph, offset);
        }
    }
    canvas.rgba()
}

fn frame(font: &Font, canvas: &mut Canvas, size: (i32, i32)) {
    let (w, h) = size;
    for y in 0..h {
        for x in 0..w {
            canvas.put(MARGIN + x, MARGIN + y, |back| back.map(|c| c / 2));
        }
    }
    for y in 0..h {
        for x in 0..w {
            let index = font.texel((x as usize) % TILE_SIDE, TILE_TOP + (y as usize) % TILE_SIDE);
            canvas.texel(MARGIN + x, MARGIN + y, font.frame_word(index), FRAME_COLOR, Blend::Average);
        }
    }
    rectangle(canvas, (MARGIN - 1, MARGIN - 1, MARGIN + w, MARGIN + h), OUTLINE_GREY);
    rectangle(canvas, (MARGIN - 2, MARGIN - 2, MARGIN + w + 1, MARGIN + h + 1), OUTER_GREY);
}

fn rectangle(canvas: &mut Canvas, (left, top, right, bottom): (i32, i32, i32, i32), grey: u8) {
    let level = u32::from(grey >> 3);
    for x in left..=right {
        canvas.put(x, top, |_| [level; 3]);
        canvas.put(x, bottom, |_| [level; 3]);
    }
    for y in top..=bottom {
        canvas.put(left, y, |_| [level; 3]);
        canvas.put(right, y, |_| [level; 3]);
    }
}

fn draw(font: &Font, canvas: &mut Canvas, glyph: &Glyph, offset: (i32, i32)) {
    let (u, v) = match glyph.shape {
        Shape::Byte(byte) => message_cell(byte),
        Shape::Number(number) => number_cell(number),
    };
    for row in 0..CELL_HEIGHT {
        for column in 0..CELL_WIDTH.min(GLYPH_ADVANCE as usize) {
            let word = font.text_word(glyph.color, font.texel(u + column, v + row));
            canvas.texel(
                offset.0 + glyph.x + column as i32,
                offset.1 + glyph.y + row as i32,
                word,
                NEUTRAL,
                Blend::Add,
            );
        }
    }
}
