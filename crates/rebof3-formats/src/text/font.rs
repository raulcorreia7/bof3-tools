//! The message font: the glyph sheet the game uploads to the font texture page, the cell each byte reads and
//! the palettes the text and the window frame draw with.
//!
//! - The sheet is `ETC/FIRST.EMI` entry 3 (load argument `0x1E000200`): VRAM column 30 (x 960), row 0, two
//!   chunks per row, so a 256 by 256 texel 4-bit page. It is the font page of `FONT_TPAGE_X` and
//!   `FONT_TPAGE_Y` in `include/gfx/prim.h`, drawn with texture page `0x2F` by `func_8014FC00
//!   (exe/slus_004_22@0x8014FC00)` (column 15, 4-bit, blend mode 1).
//! - The text palette is `FIRST.EMI` entry 8: the stage palette's row 0 (`0x80033800` copied to CLUT row 480 by
//!   `GFX_CopyPalette`/`GFX_UploadPalette`). The glyph CLUT word is `0x7800 | n`, so cell `n` is words
//!   `16n..16n+16` (`UI_DrawText (exe/slus_004_22@0x8014FEA4)`).
//! - The frame palette is `FIRST.EMI` entry 9 (`0x80033A00`, CLUT row 481); window style 0 reads its cell 1
//!   (`UI_DrawWindow`: CLUT `(style << 5 | 0x10, 481)`).

use psx_formats::gfx::texture::{Depth, texel_indices};
use psx_formats::gfx::vram::{Descriptor, word_grid};

/// The glyph cell, in pixels: `UI_DrawTextGlyphs (exe/slus_004_22@0x8014F84C)` draws 8 by 12.
pub const CELL_WIDTH: usize = 8;
pub const CELL_HEIGHT: usize = 12;
/// Glyph columns per row of the message sheet (`% 0x1F`) and the byte of the first glyph (`c - 0x30`).
const MESSAGE_COLUMNS: i32 = 31;
const MESSAGE_FIRST: i32 = 0x30;
/// The sheet row the message glyphs start at (`+ 0x48`).
const MESSAGE_TOP: i32 = 0x48;
/// The number sheet: 21 columns, 12 pixels apart, indexed from `n + 0x5B`.
const NUMBER_COLUMNS: i32 = 21;
const NUMBER_PITCH: i32 = 12;
const NUMBER_BIAS: i32 = 0x5B;
/// The frame tile's top-left on the font page: 16 by 16 texels at `v = 240` (`UI_DrawWindow` texture window).
pub const TILE_TOP: usize = 240;
pub const TILE_SIDE: usize = 16;
/// Words per palette cell, and the frame palette's cell for window style 0.
pub const CELL_WORDS: usize = 16;
const FRAME_CELL: usize = 1;

/// The font page, the text palette and the frame palette.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Font {
    width: usize,
    texels: Vec<u8>,
    text: Vec<u16>,
    frame: Vec<u16>,
}

impl Font {
    /// Builds the font from the three `FIRST.EMI` payloads. `sheet_load_argument` is the sheet entry's load
    /// argument. Returns `None` when the sheet is not a whole page or a palette is shorter than a cell.
    pub fn new(sheet: &[u8], sheet_load_argument: u32, text_palette: &[u8], frame_palette: &[u8]) -> Option<Self> {
        let grid = word_grid(sheet, Descriptor::from_load_argument(sheet_load_argument)).ok()?;
        let texels = texel_indices(&grid.words, Depth::Four);
        let width = grid.width * Depth::Four.texels_per_word();
        let (text, frame) = (words(text_palette), words(frame_palette));
        (texels.len() >= width * (TILE_TOP + TILE_SIDE) && frame.len() >= (FRAME_CELL + 1) * CELL_WORDS)
            .then_some(Self { width, texels, text, frame })
    }

    /// The texel index at `(u, v)` of the font page.
    pub fn texel(&self, u: usize, v: usize) -> u8 {
        self.texels.get(v * self.width + u).copied().unwrap_or(0)
    }

    /// The colour word of text palette cell `cell` (its low four bits) at `index`.
    pub fn text_word(&self, cell: u8, index: u8) -> u16 {
        let at = usize::from(cell & 0x0F) * CELL_WORDS + usize::from(index);
        self.text.get(at).copied().unwrap_or(0)
    }

    /// The colour word of the window frame palette at `index`.
    pub fn frame_word(&self, index: u8) -> u16 {
        self.frame[FRAME_CELL * CELL_WORDS + usize::from(index)]
    }

    /// Whether the glyph cell of message byte `byte` holds any texel: the font draws something for it.
    pub fn has_glyph(&self, byte: u8) -> bool {
        let (u, v) = message_cell(byte);
        (0..CELL_HEIGHT).any(|row| (0..CELL_WIDTH).any(|column| self.texel(u + column, v + row) != 0))
    }
}

fn words(bytes: &[u8]) -> Vec<u16> {
    bytes.as_chunks::<2>().0.iter().map(|pair| u16::from_le_bytes(*pair)).collect()
}

/// The sheet position of the cell a message byte draws: `u = ((c - 0x30) % 31) * 8` and
/// `v = ((c - 0x30) / 31) * 12 + 0x48` in C integer arithmetic, each stored in a byte
/// (`UI_DrawTextGlyphs (exe/slus_004_22@0x8014F84C)`). Bytes below `0x30` give negative terms the byte wraps.
pub fn message_cell(byte: u8) -> (usize, usize) {
    let index = i32::from(byte) - MESSAGE_FIRST;
    let u = (index % MESSAGE_COLUMNS) * CELL_WIDTH as i32;
    let v = (index / MESSAGE_COLUMNS) * CELL_HEIGHT as i32 + MESSAGE_TOP;
    (usize::from(u as u8), usize::from(v as u8))
}

/// The sheet position of the cell `{NUM:n}` draws (`UI_DrawMessageScript`, case `STRING_NUM`).
pub fn number_cell(number: u8) -> (usize, usize) {
    let index = i32::from(number) + NUMBER_BIAS;
    let u = (index % NUMBER_COLUMNS) * NUMBER_PITCH;
    let v = (index / NUMBER_COLUMNS) * NUMBER_PITCH;
    (usize::from(u as u8), usize::from(v as u8))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_cells_follow_the_game_formula() {
        assert_eq!(message_cell(0x30), (0, 0x48));
        assert_eq!(message_cell(0x4E), (30 * 8, 0x48));
        assert_eq!(message_cell(0x4F), (0, 0x48 + 12));
        // Below the first glyph the terms go negative and the byte stores wrap: 0x12 gives u 16, v 72.
        assert_eq!(message_cell(0x12), (16, 72));
    }

    #[test]
    fn number_cells_start_at_the_bias() {
        assert_eq!(number_cell(0), (((0x5B) % 21 * 12) as usize, ((0x5B) / 21 * 12) as usize));
    }
}
