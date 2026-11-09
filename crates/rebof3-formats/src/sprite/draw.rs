//! A sprite frame drawn as the field draws it: the sheet and palette staged into the VRAM model where
//! the field reader puts them, then each cell's texels fetched back through the texture page and CLUT
//! the draw sets.
//!
//! Where a field actor's sprite goes is set by `Party_LoadMemberSprite` (`emi/etc/game/00@0x801C1254`) for the
//! party member in slot `s`: texture page 5 (work `+0x25`), sheet row `s * 0x50` (`+0x26`), CLUT
//! selector `s + 0x78` (`+0x27`), palette kind 2 (`+0x28`), and `GFX_LoadPalette(0x80035600 + s * 0x40,
//! 0)`. From those:
//!
//! - `Actor_SelectScriptBank` (`exe/slus_004_22@0x8014DC08`) queues the sheet at VRAM word
//!   `page << 6`, row `+0x26` (a page above 16 moves to the lower half).
//! - `Area_DrawObjectSprite` (`exe/slus_004_22@0x8014C4D4`) sets the texture page from `+0x25` and the
//!   8-bit mode from `+0x28`, adds `+0x26` to each cell's `v`, and takes the CLUT from
//!   `Area_BuildClut` (`exe/slus_004_22@0x8014CE78`), which for kind 2 is row `480 + (sel >> 3)`,
//!   column `(sel & 7) * 32`: the arena row and column `GFX_LoadPalette` wrote.
//!
//! Battle character sprites (`BPLD`, `BPLU`) are read the same way. `Battle_LoadPartySprites`
//! (`emi/etc/game/00@0x80197AA4`) streams `BPLD` or `BPLU` to `0x8003B800` by `g_Battle.scroll_dir & 2`;
//! `Battle_LoadBattleArchive` (`emi/etc/game/00@0x80197C94`) then runs `Party_BuildBattlers(PARTY_ORDER_BATTLE)`,
//! which calls `Party_LoadMemberSprite` for every battle slot. The battle overlay reloads character sprites mid-battle
//! (`func_801E7948`, `emi/battle/battle/03@0x801E7948`) and stages palette 0 again for the slot at
//! `GAME_PALETTE_STAGE + slot * 0x40` (`func_801E7634`, `func_801E7BA0`, `func_801E8684`). The slot, and
//! so the sheet row and CLUT, is the member's place in the battle order, a run-time choice that moves the
//! sprite in VRAM but not its colours; a status fade (`func_801DDAB4`, status bit `0x80`) tints it at run
//! time.
//!
//! A texel outside the uploaded sheet, or a colour outside the staged palette, depends on what else the
//! frame's VRAM holds at run time; the composite marks it instead of guessing it.

use psx_formats::gfx::texture::{Clut, Depth};

use super::{Animation, Cell, Frame, Image, Palette};
use crate::error::{Error, Result};
use rebof3_emi::replay::{Kind, Vram, Writer};
use rebof3_emi::texture::ARENA_VRAM_ROW;

/// The address of `g_SpriteSizes` (`exe/slus_004_22@0x8017F53C`): one width and height per shape.
pub const SPRITE_SIZES: u32 = 0x8017_F53C;
/// Shapes in `g_SpriteSizes`.
pub const SHAPES: usize = 16;
/// The PS-X EXE magic, the header word that holds the text address, and where the text starts.
const EXE_MAGIC: &[u8; 8] = b"PS-X EXE";
const EXE_TEXT_ADDRESS: usize = 0x18;
const EXE_TEXT_START: usize = 0x800;

/// The texture page `Party_LoadMemberSprite` gives a field actor (`unk_25 = 5`).
pub const FIELD_PAGE: u8 = 5;
/// Sheet rows each party slot owns (`unk_26 = slot * 0x50`).
pub const FIELD_SLOT_ROWS: u8 = 0x50;
/// The CLUT selector of party slot 0 (`unk_27 = slot + 0x78`).
pub const FIELD_CLUT_SELECTOR: u8 = 0x78;
/// The RAM `GFX_LoadPalette` stages party slot 0's palette at (`GAME_PALETTE_STAGE`), and the stride.
pub const FIELD_PALETTE_STAGE: u32 = 0x8003_5600;
const FIELD_PALETTE_STRIDE: u32 = 0x40;
/// The palette `Party_LoadMemberSprite` stages: `GFX_LoadPalette(..., 0)`.
pub const FIELD_PALETTE: usize = 0;

/// `page << 6`: a texture page is 64 words wide.
const PAGE_WORDS_SHIFT: u32 = 6;
/// Pages in one row of VRAM, and `Actor_SelectScriptBank`'s test for the lower half (`level > 16`).
const PAGES_PER_ROW: u8 = 16;
/// The row a lower-half page starts at, and the tpage's row shift (`(page / 16) << 8`).
const LOWER_HALF_ROW: u16 = 0x100;
const PAGE_ROW_SHIFT: u32 = 8;
/// `Area_BuildClut`'s palette kinds (work `+0x28`, `PALETTE_KIND_*` in `include/gfx/vram.h`).
pub const KIND_CELL: u8 = 0;
pub const KIND_ROW: u8 = 1;
pub const KIND_CELLS_2: u8 = 2;
pub const KIND_CELLS_4: u8 = 3;
/// `Area_BuildClut` starts at row `STAGE_PALETTE_Y + 16` when work `+0x24` has this bit.
pub const UPPER_PALETTES: u8 = 0x04;
const UPPER_PALETTE_ROWS: u16 = 16;
/// Words in one CLUT unit: a CLUT column counts in 16-word steps.
const CLUT_ROW_SHIFT: u32 = 6;
/// The work `+0x24` flags `Party_LoadMemberSprite` sets (`active_flags = 1`).
const FIELD_FLAGS: u8 = 1;
/// The colour word the GPU never draws.
pub const TRANSPARENT: u16 = 0;

/// `g_SpriteSizes`: the width and height of each cell shape, in texels.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpriteSizes {
    rows: [[u8; 2]; SHAPES],
}

impl SpriteSizes {
    /// The table from its rows.
    pub fn from_rows(rows: [[u8; 2]; SHAPES]) -> Self {
        Self { rows }
    }

    /// The table as the executable (`SLUS_004.22`) stores it.
    pub fn from_executable(exe: &[u8]) -> Result<Self> {
        let not_exe = |why: &str| Error::CharacterSprites(format!("the executable has no g_SpriteSizes: {why}"));
        if exe.get(..EXE_MAGIC.len()) != Some(&EXE_MAGIC[..]) {
            return Err(not_exe("it is not a PS-X EXE"));
        }
        let text = exe.get(EXE_TEXT_ADDRESS..EXE_TEXT_ADDRESS + 4).map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]])).ok_or_else(|| not_exe("its header is short"))?;
        let offset = SPRITE_SIZES.checked_sub(text).map(|offset| EXE_TEXT_START + offset as usize).ok_or_else(|| not_exe("its text starts after the table"))?;
        let bytes = exe.get(offset..offset + 2 * SHAPES).ok_or_else(|| not_exe("its text ends before the table"))?;
        let mut rows = [[0; 2]; SHAPES];
        for (row, pair) in rows.iter_mut().zip(bytes.as_chunks::<2>().0) {
            *row = *pair;
        }
        Ok(Self { rows })
    }

    /// The width and height of `cell`.
    pub fn size(&self, cell: Cell) -> (usize, usize) {
        let [width, height] = self.rows[cell.shape()];
        (usize::from(width), usize::from(height))
    }
}

/// The actor work fields that place a sprite in VRAM.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Placement {
    /// The texture page (work `+0x25`).
    pub page: u8,
    /// The sheet's first VRAM row, which every cell's `v` counts from (work `+0x26`).
    pub row: u8,
    /// The CLUT selector `Area_BuildClut` reads (work `+0x27`).
    pub clut_selector: u8,
    /// The RAM `GFX_LoadPalette` writes the palette to.
    pub palette_stage: u32,
    /// The palette kind `Area_BuildClut` and the tpage depth read (work `+0x28`).
    pub kind: u8,
    /// The work flags whose [`UPPER_PALETTES`] bit moves the CLUT 16 rows down (work `+0x24`).
    pub flags: u8,
}

impl Placement {
    /// The placement `Party_LoadMemberSprite` gives party slot `slot` in the field.
    pub fn field(slot: u8) -> Self {
        Self {
            page: FIELD_PAGE,
            row: slot.wrapping_mul(FIELD_SLOT_ROWS),
            clut_selector: FIELD_CLUT_SELECTOR.wrapping_add(slot),
            palette_stage: FIELD_PALETTE_STAGE + u32::from(slot) * FIELD_PALETTE_STRIDE,
            kind: KIND_CELLS_2,
            flags: FIELD_FLAGS,
        }
    }

    /// The placement `Area_FindSceneRecord` (`exe/slus_004_22@0x8014DD3C`) copies from an area's scene
    /// record: texture page, sheet row, CLUT selector and palette kind. Its sheet is the area's own
    /// upload, so it stages no palette.
    pub fn scene(page: u8, row: u8, clut_selector: u8, kind: u8, flags: u8) -> Self {
        Self { page, row, clut_selector, palette_stage: 0, kind, flags }
    }

    /// Where `Actor_SelectScriptBank` queues the sheet: VRAM word column and row.
    pub fn sheet_origin(&self) -> (usize, usize) {
        let (page, row) = if self.page > PAGES_PER_ROW { (self.page - PAGES_PER_ROW, u16::from(self.row) + LOWER_HALF_ROW) } else { (self.page, u16::from(self.row)) };
        (usize::from(page) << PAGE_WORDS_SHIFT, usize::from(row))
    }

    /// The texture page `Area_DrawObjectSprite` sets: its top-left word.
    pub fn texture_page(&self) -> (usize, usize) {
        let column = usize::from(self.page % PAGES_PER_ROW) << PAGE_WORDS_SHIFT;
        let row = usize::from(self.page / PAGES_PER_ROW) << PAGE_ROW_SHIFT;
        (column, row)
    }

    /// The CLUT `Area_BuildClut` (`exe/slus_004_22@0x8014CE78`) builds: the row is the base plus the
    /// selector's high bits, the column in 16-word units its low bits, split by the palette kind.
    pub fn clut(&self) -> Clut {
        let base = ARENA_VRAM_ROW + if self.flags & UPPER_PALETTES != 0 { UPPER_PALETTE_ROWS } else { 0 };
        let selector = u16::from(self.clut_selector);
        let (row, units) = match self.kind {
            KIND_CELL => (selector >> 4, selector & 0xF),
            KIND_ROW => (selector, 0),
            KIND_CELLS_2 => (selector >> 3, (selector & 7) * 2),
            KIND_CELLS_4 => (selector >> 2, (selector & 3) * 4),
            _ => (selector >> 1, (selector & 1) * 8),
        };
        Clut::from_word((base + row) << CLUT_ROW_SHIFT | units)
    }

    /// The texel depth `Area_DrawObjectSprite` sets: 8-bit for a nonzero palette kind, else 4-bit.
    pub fn depth(&self) -> Depth {
        if self.kind == KIND_CELL { Depth::Four } else { Depth::Eight }
    }
}

/// The code that reads character sprites, by the family of the archive that streams it.
///
/// `EMI_GetArchiveId` (`exe/slus_004_22@0x8016728C`) streams family 0 from `EMI_SLOT_PL`
/// (`BIN/PLCHAR/PLnnn.EMI`) and families 1 and 2 from `EMI_SLOT_BPLD` and `EMI_SLOT_BPLU`
/// (`BIN/BPLCHAR/BPLDnnn.EMI`, `BPLUnnn.EMI`), the ones `func_801E7948` (`emi/battle/battle/03@0x801E7948`)
/// asks for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Reader {
    /// Party character sprites: `Party_LoadMemberSprite` places it and loads palette [`FIELD_PALETTE`], called from
    /// `Party_JoinMember` and `Party_BuildBattlers` (`emi/etc/game/00`).
    Field,
    /// Battle character sprites: `Battle_LoadPartySprites` streams it and `Party_BuildBattlers(PARTY_ORDER_BATTLE)`
    /// places it through `Party_LoadMemberSprite`, palette [`FIELD_PALETTE`]; see the module documentation.
    Battle,
    /// No traced loader streams the archive.
    Unknown,
}

impl Reader {
    /// The family of the archive at `path` (`BIN/…/NAME.EMI`).
    pub fn of(path: &str) -> Self {
        let name = path.rsplit('/').next().unwrap_or(path);
        let stem = name.split_once('.').map_or(name, |(stem, _)| stem);
        let numbered = |prefix: &str| stem.strip_prefix(prefix).is_some_and(|rest| rest.starts_with(|first: char| first.is_ascii_digit()));
        if numbered(PARTY_FIELD) {
            Reader::Field
        } else if numbered(PARTY_BATTLE_DOWN) || numbered(PARTY_BATTLE_UP) {
            Reader::Battle
        } else {
            Reader::Unknown
        }
    }

    /// The palette the reader loads, when it is traced.
    pub fn palette(self) -> Option<usize> {
        matches!(self, Reader::Field | Reader::Battle).then_some(FIELD_PALETTE)
    }
}

/// The archive name prefixes of the party families `EMI_GetArchiveId` streams.
const PARTY_FIELD: &str = "PL";
const PARTY_BATTLE_DOWN: &str = "BPLD";
const PARTY_BATTLE_UP: &str = "BPLU";

impl Default for Placement {
    fn default() -> Self {
        Self::field(0)
    }
}

/// One texel a cell reads: its index, its colour, and whether both came from the staged character sprites.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Texel {
    /// The 8-bit index in the sheet.
    pub index: u8,
    /// The colour word the CLUT gives the index.
    pub word: u16,
    /// Whether the staged sheet wrote the texel's VRAM word.
    pub in_sheet: bool,
    /// Whether the staged palette wrote the CLUT word.
    pub in_palette: bool,
}

impl Texel {
    /// Whether the GPU draws the texel: a colour the palette gives is drawn unless it is `0x0000`; a
    /// colour from outside the palette is unknown and counts as drawn.
    pub fn drawn(&self) -> bool {
        self.in_sheet && (!self.in_palette || self.word != TRANSPARENT)
    }
}

/// The VRAM of one actor: a sheet and a palette at their placement.
#[derive(Clone)]
pub struct Stage {
    /// The frame buffer with the sheet and the palette written.
    pub vram: Vram,
    /// Where they went.
    pub placement: Placement,
    /// The writers of the staged sheet and palette; `None` reads what a scene's loads left there.
    staged: Option<(Writer, Writer)>,
}

impl Stage {
    /// Writes sheet `sheet` and palette `palette` of a section into `vram` as the field does: the palette
    /// with `GFX_LoadPalette` and the next `GFX_UploadPalette`, the sheet with the transfer queue's
    /// `LoadImage`. The numbers name the writers.
    pub fn new(mut vram: Vram, sheet: (usize, &Image), palette: (usize, &Palette), placement: Placement) -> Self {
        let ((sheet, image), (palette, colours)) = (sheet, palette);
        let palette_writer = Writer::Named(format!("sprite palette {palette}"));
        vram.load_palette(placement.palette_stage, colours, palette_writer.clone());
        vram.upload_palette();

        let sheet_writer = Writer::Named(format!("sprite frame sheet {sheet}"));
        let (x, y) = placement.sheet_origin();
        vram.load_image(x, y, usize::from(image.width), usize::from(image.height), &image.words(), sheet_writer.clone());
        Self { vram, placement, staged: Some((sheet_writer, palette_writer)) }
    }

    /// An actor whose sheet and palette are already in `vram`, a replayed scene: an area actor placed by
    /// its scene record. A texel counts as the sheet's when an upload wrote it, and a colour as the
    /// palette's when any load wrote it.
    pub fn resident(vram: Vram, placement: Placement) -> Self {
        Self { vram, placement, staged: None }
    }

    /// The texel the draw reads at sheet column `u` and cell row `v`, as `Area_DrawObjectSprite` forms
    /// it: the page column plus `u` at the placement's depth, the page row plus `+0x26` plus `v`, both
    /// within one byte.
    pub fn texel(&self, u: u8, v: u8) -> Texel {
        let (page_x, page_y) = self.placement.texture_page();
        let per_word = self.placement.depth().texels_per_word();
        let bits = self.placement.depth().bits() as usize;
        let column = page_x + usize::from(u) / per_word;
        let row = page_y + usize::from(self.placement.row.wrapping_add(v));
        let shift = (usize::from(u) % per_word) * bits;
        let index = ((self.vram.word(column, row) >> shift) & ((1 << bits) - 1)) as u8;

        let clut = self.placement.clut();
        let (clut_x, clut_y) = (usize::from(clut.x) + usize::from(index), usize::from(clut.y));
        let (in_sheet, in_palette) = match &self.staged {
            Some((sheet, palette)) => (self.vram.writer(column, row) == Some(sheet), self.vram.writer(clut_x, clut_y) == Some(palette)),
            None => (self.vram.kind(column, row) == Some(Kind::Upload), self.vram.writer(clut_x, clut_y).is_some()),
        };
        Texel { index, word: self.vram.word(clut_x, clut_y), in_sheet, in_palette }
    }
}

/// A rectangle around the actor's position: left and top inclusive, right and bottom exclusive.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Bounds {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Bounds {
    pub fn width(&self) -> usize {
        (self.right - self.left).max(0) as usize
    }

    pub fn height(&self) -> usize {
        (self.bottom - self.top).max(0) as usize
    }

    /// The smallest rectangle around both.
    pub fn union(self, other: Self) -> Self {
        Self { left: self.left.min(other.left), top: self.top.min(other.top), right: self.right.max(other.right), bottom: self.bottom.max(other.bottom) }
    }
}

/// Where a cell lands on screen: its rectangle and whether its texels run right to left.
pub fn cell_rect(cell: Cell, sizes: &SpriteSizes, mirror: bool) -> (Bounds, bool) {
    let (width, height) = sizes.size(cell);
    let (width, height) = (width as i32, height as i32);
    let left = if mirror { -i32::from(cell.x) - width } else { i32::from(cell.x) };
    let top = i32::from(cell.y);
    let bounds = Bounds { left, top, right: left + width, bottom: top + height };
    (bounds, cell.flipped() != mirror)
}

/// The rectangle around a frame's cells, when it has any.
pub fn frame_bounds(frame: &Frame, sizes: &SpriteSizes, mirror: bool) -> Option<Bounds> {
    frame.cells.iter().map(|&cell| cell_rect(cell, sizes, mirror).0).reduce(Bounds::union)
}

/// The rectangle around every frame of `animation`.
pub fn animation_bounds(animation: &Animation, sizes: &SpriteSizes, mirror: bool) -> Option<Bounds> {
    animation.frames.iter().filter_map(|frame| frame_bounds(frame, sizes, mirror)).reduce(Bounds::union)
}

/// One pixel of a composed frame: the cell and sheet texel it shows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Pixel {
    /// The cell, in draw order.
    pub cell: usize,
    /// The sheet texel.
    pub u: u8,
    pub v: u8,
    pub texel: Texel,
}

/// A frame composed over `bounds`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Composite {
    pub bounds: Bounds,
    /// Row-major over `bounds`: the top drawn texel, or, where every cell's texel is transparent, the
    /// top cell's texel; `None` where no cell reaches from the sheet.
    pub pixels: Vec<Option<Pixel>>,
    /// Cell texels that fall outside the sheet, which the frame's other VRAM supplies at run time.
    pub outside: usize,
}

impl Composite {
    /// The pixel at `x`, `y` of the bounds.
    pub fn pixel(&self, x: usize, y: usize) -> Option<Pixel> {
        (x < self.bounds.width()).then(|| self.pixels.get(y * self.bounds.width() + x).copied().flatten()).flatten()
    }

    /// Pixels whose colour the staged palette does not give.
    pub fn unresolved_colours(&self) -> usize {
        self.pixels.iter().flatten().filter(|pixel| pixel.texel.drawn() && !pixel.texel.in_palette).count()
    }
}

/// `frame` drawn from `stage` in cell order, over `bounds`.
pub fn compose(stage: &Stage, frame: &Frame, sizes: &SpriteSizes, mirror: bool, bounds: Bounds) -> Composite {
    let (width, height) = (bounds.width(), bounds.height());
    let mut pixels: Vec<Option<Pixel>> = vec![None; width * height];
    let mut outside = 0;

    for (index, &cell) in frame.cells.iter().enumerate() {
        let (rect, reversed) = cell_rect(cell, sizes, mirror);
        let cell_width = rect.width();
        for row in 0..rect.height() {
            for column in 0..cell_width {
                let (x, y) = (rect.left + column as i32 - bounds.left, rect.top + row as i32 - bounds.top);
                let step = if reversed { cell_width - 1 - column } else { column };
                let (u, v) = (cell.u.wrapping_add(step as u8), cell.v.wrapping_add(row as u8));
                let texel = stage.texel(u, v);
                if !texel.in_sheet {
                    outside += 1;
                    continue;
                }
                if x < 0 || y < 0 || x as usize >= width || y as usize >= height {
                    continue;
                }
                let slot = &mut pixels[y as usize * width + x as usize];
                if texel.drawn() || !slot.is_some_and(|pixel| pixel.texel.drawn()) {
                    *slot = Some(Pixel { cell: index, u, v, texel });
                }
            }
        }
    }
    Composite { bounds, pixels, outside }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sprite::{COLOURS, Encoding};

    /// `g_SpriteSizes` as the retail executable holds it: widths 8 to 32 by the low two bits, heights by
    /// the next two.
    pub(crate) fn sizes() -> SpriteSizes {
        let mut rows = [[0; 2]; SHAPES];
        for (shape, row) in rows.iter_mut().enumerate() {
            *row = [8 * (shape as u8 % 4 + 1), 8 * (shape as u8 / 4 + 1)];
        }
        SpriteSizes::from_rows(rows)
    }

    fn palette() -> Palette {
        let mut palette = [0u16; COLOURS];
        for (index, colour) in palette.iter_mut().enumerate().skip(1) {
            *colour = index as u16 * 0x21;
        }
        palette
    }

    #[test]
    fn the_field_palette_lands_on_the_clut_the_draw_builds() {
        for slot in 0..3 {
            let placement = Placement::field(slot);
            let clut = placement.clut();
            let arena_word = (placement.palette_stage - rebof3_emi::texture::ARENA_BASE) / 2;
            let row_words = u32::from(rebof3_emi::texture::ARENA_ROW_WORDS);
            assert_eq!(u32::from(clut.y - ARENA_VRAM_ROW), arena_word / row_words, "slot {slot} row");
            assert_eq!(u32::from(clut.x), arena_word % row_words, "slot {slot} column");
        }
    }

    #[test]
    fn the_sheet_lands_on_the_page_the_draw_reads() {
        let placement = Placement::field(1);
        let (x, y) = placement.sheet_origin();
        assert_eq!((x, y), (320, 0x50));
        assert_eq!(placement.texture_page(), (320, 0));
    }

    #[test]
    fn a_frame_draws_its_cells_in_order_and_flips() {
        // A 16x8 sheet: texel (u, v) holds u + 1, so every texel is drawn.
        let texels: Vec<u8> = (0..8).flat_map(|_| (1..=16u8).collect::<Vec<_>>()).collect();
        let image = Image::new(8, 8, Encoding::Raw, texels).expect("a sheet");
        let stage = Stage::new(Vram::new(), (0, &image), (0, &palette()), Placement::field(0));
        let sizes = sizes();

        let plain = Cell { size: 0, x: 0, y: 0, u: 0, v: 0 };
        let flipped = Cell { size: crate::animation::FLIP, x: 8, y: 0, u: 0, v: 0 };
        let frame = Frame { cells: vec![plain, flipped] };
        let bounds = frame_bounds(&frame, &sizes, false).expect("cells");
        assert_eq!(bounds, Bounds { left: 0, top: 0, right: 16, bottom: 8 });

        let composite = compose(&stage, &frame, &sizes, false, bounds);
        assert_eq!(composite.pixel(0, 0).map(|pixel| pixel.texel.index), Some(1));
        assert_eq!(composite.pixel(8, 0).map(|pixel| pixel.texel.index), Some(8), "flipped: u 7 first");
        assert_eq!(composite.pixel(15, 0).map(|pixel| pixel.texel.index), Some(1));
        assert_eq!(composite.outside, 0);

        let mirrored = compose(&stage, &frame, &sizes, true, frame_bounds(&frame, &sizes, true).expect("cells"));
        assert_eq!(mirrored.bounds, Bounds { left: -16, top: 0, right: 0, bottom: 8 });
        assert_eq!(mirrored.pixel(8, 0).map(|pixel| pixel.texel.index), Some(8), "mirrored: u 7 at the left");
        assert_eq!(mirrored.pixel(0, 0).map(|pixel| pixel.texel.index), Some(1), "mirrored and flipped: plain");
    }

    #[test]
    fn a_transparent_texel_shows_the_cell_under_it() {
        let mut texels = vec![3u8; 16 * 8];
        texels[0] = 0;
        let image = Image::new(8, 8, Encoding::Raw, texels).expect("a sheet");
        let stage = Stage::new(Vram::new(), (0, &image), (0, &palette()), Placement::field(0));
        let under = Cell { size: 0, x: 0, y: 0, u: 8, v: 0 };
        let over = Cell { size: 0, x: 0, y: 0, u: 0, v: 0 };
        let frame = Frame { cells: vec![under, over] };
        let composite = compose(&stage, &frame, &sizes(), false, frame_bounds(&frame, &sizes(), false).unwrap());
        assert_eq!(composite.pixel(0, 0).map(|pixel| pixel.cell), Some(0), "the top cell's texel is index 0");
        assert_eq!(composite.pixel(1, 0).map(|pixel| pixel.cell), Some(1));
    }

    #[test]
    fn a_texel_past_the_sheet_or_the_palette_is_marked() {
        let mut texels = vec![1u8; 16 * 8];
        texels[1] = 40;
        let image = Image::new(8, 8, Encoding::Raw, texels).expect("a sheet");
        let stage = Stage::new(Vram::new(), (0, &image), (0, &palette()), Placement::field(0));
        let frame = Frame { cells: vec![Cell { size: 4, x: 0, y: 0, u: 0, v: 0 }] };
        let composite = compose(&stage, &frame, &sizes(), false, frame_bounds(&frame, &sizes(), false).unwrap());
        assert_eq!(composite.outside, 8 * 8, "rows 8 to 15 are past the 8-row sheet");
        assert_eq!(composite.unresolved_colours(), 1, "index 40 is past the 32-colour palette");
    }

    #[test]
    fn a_scene_placement_builds_every_clut_kind() {
        // Kind 0: row base + sel >> 4, column (sel & 15) * 16, 4-bit; the 0x04 flag moves it 16 rows down.
        let cell = Placement::scene(7, 0x40, 0xB3, KIND_CELL, UPPER_PALETTES);
        assert_eq!((cell.clut().x, cell.clut().y), (48, 496 + 11));
        assert_eq!(cell.depth(), Depth::Four);
        let row = Placement::scene(7, 0, 5, KIND_ROW, 0);
        assert_eq!((row.clut().x, row.clut().y), (0, 485));
        let four = Placement::scene(7, 0, 0x0B, KIND_CELLS_4, 0);
        assert_eq!((four.clut().x, four.clut().y, four.depth()), (192, 482, Depth::Eight));
        let eight = Placement::scene(7, 0, 0x0B, 4, 0);
        assert_eq!((eight.clut().x, eight.clut().y), (128, 485));
    }

    #[test]
    fn a_resident_stage_reads_4_bit_texels_any_upload_wrote() {
        let mut vram = Vram::new();
        // Page 7 is VRAM column 448; texel u 5 is the second nibble of word 1.
        let sheet: Vec<u8> = vec![0x21; 2048];
        vram.load("BIN/WORLD00/AREA003.EMI", &[rebof3_emi::Entry { size: 2048, offset: 0, ram_ptr: (14 << 24) | (1 << 8), first4: 0, file_type: 3 }], &[&sheet]);
        let stage = Stage::resident(vram, Placement::scene(7, 0, 0, KIND_CELL, 0));
        let texel = stage.texel(5, 3);
        assert_eq!(texel.index, 2);
        assert!(texel.in_sheet);
        assert!(!texel.in_palette, "no load wrote the CLUT row");
        assert!(!stage.texel(0, 40).in_sheet, "row 40 is past the 32-row upload");
    }

    #[test]
    fn sizes_read_from_an_executable() {
        let text = 0x8001_0000u32;
        let mut exe = vec![0u8; EXE_TEXT_START + (SPRITE_SIZES - text) as usize + 2 * SHAPES];
        exe[..8].copy_from_slice(EXE_MAGIC);
        exe[EXE_TEXT_ADDRESS..EXE_TEXT_ADDRESS + 4].copy_from_slice(&text.to_le_bytes());
        let at = EXE_TEXT_START + (SPRITE_SIZES - text) as usize;
        exe[at..at + 2].copy_from_slice(&[24, 16]);
        let sizes = SpriteSizes::from_executable(&exe).expect("reads");
        assert_eq!(sizes.size(Cell { size: 0x80, x: 0, y: 0, u: 0, v: 0 }), (24, 16));
        assert!(SpriteSizes::from_executable(b"not an exe").is_err());
    }

    #[test]
    fn the_archive_family_names_the_reader() {
        assert_eq!(Reader::of("BIN/PLCHAR/PL012.EMI"), Reader::Field);
        assert_eq!(Reader::of("BIN/BPLCHAR/BPLD012.EMI"), Reader::Battle);
        assert_eq!(Reader::of("BIN/BPLCHAR/BPLU27A.EMI"), Reader::Battle);
        assert_eq!(Reader::of("BIN/ETC/START.EMI"), Reader::Unknown);
        assert_eq!(Reader::of("BIN/ETC/PLATE.EMI"), Reader::Unknown, "a name that only starts like a family");
        assert_eq!(Reader::Field.palette(), Some(FIELD_PALETTE));
        assert_eq!(Reader::Battle.palette(), Some(FIELD_PALETTE));
        assert_eq!(Reader::Unknown.palette(), None);
    }
}
