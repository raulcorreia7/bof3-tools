//! The terrain of a map bundle as the field renderer draws it: one textured quad per drawn cell, plus
//! the east and south walls where a neighbour is lower.
//!
//! The mesh builder `func_801530D0` (`exe/slus_004_22@0x801530D0`) walks the visible cells, projects
//! each cell's four corners and links one `POLY_FT4` per cell into the ordering table. Two more rows
//! hang off a cell (`+0x46` and `+0x4E` of its primitive row):
//!
//! - the **east wall**, allocated when the east neighbour's corner 0 is below this cell's corner 1 or
//!   its corner 2 is below this cell's corner 3 (`@0x801534B4`-`@0x801534D8`);
//! - the **south wall**, allocated when the south neighbour's corner 0 is below this cell's corner 2 or
//!   its corner 1 is below this cell's corner 3 (`@0x801534FC`-`@0x80153530`).
//!
//! `Area_UploadCell` (`@0x80154294`) gives each row its tile: the cell's grid value `g` names the
//! descriptor word `g`; the east wall takes the next word, and the south wall the word after the
//! ones already taken. A south-wall word of `0` frees the row, so no south wall is drawn.
//! `Area_GetCell` (`@0x801541C0`) reports only the cells strictly inside the bundle (column `1` to
//! `width - 2`, row `1` to `height - 2`) with a non-zero grid value, so the border is never drawn.
//!
//! The tile setter `func_80155560` (`@0x80155560`) turns a descriptor into the primitive:
//!
//! | Bits | Field | Primitive |
//! | ---- | ----- | --------- |
//! | 0-11 | the tile (block, rectangle or quad) and the bank bit | the four texture corners |
//! | 12-13 | semi-transparency mode | tpage bits 5-6 (graph type 0) |
//! | 14 | deferred to the depth-sorted list in draw modes 4 and 6 (`func_801530D0`) | none |
//! | 15 | semi-transparency on | `SetSemiTrans` |
//! | 16-18 | corner order: flip across x, flip across z, quarter turn | the texture corners |
//! | 19-23 | grey level | `r`, `g` and `b` (`(word >> 16) & 0xF8`) |
//! | 24-27 | CLUT index | CLUT word |
//! | 28-29 | page bank | tpage x |
//! | 30 | linked into the cell row's second ordering-table slot (`func_801530D0`) | none |
//! | 31 | depth: clear is 8-bit, set is 4-bit | tpage bits 7-8, CLUT word |
//!
//! The area init `func_80153DE8` (`@0x80153DE8`) sets every cell primitive with `SetShadeTex(p, 0)`,
//! so the GPU multiplies each texel by the grey level, where `0x80` leaves it unchanged.
//!
//! Positions are in the renderer's units: a cell is [`CELL_UNITS`] wide and a height step is
//! [`super::terrain::HEIGHT_SCALE`] high. The model's axes are `x` = column, `y` = up, `z` = row; the
//! game's own vertex is `(x - 0x4040, z - 0x4040, -y)` (`func_801530D0`, `AREA_CELL_ORIGIN` and
//! `AREA_CELL_HALF` in `decomp/include/area/area.h`).

use psx_formats::gfx::texture::{Clut, Depth};
use rebof3_emi::texture::{TERRAIN_CLUT_ROW, TERRAIN_PAGE_X, TERRAIN_PAGE_Y, terrain_descriptor};

use super::terrain::{CORNER_STEPS, HEIGHT_SCALE, descriptor, tile_of};
use super::{CELL_BYTES, MapBundle};

/// The width of a cell in renderer units (`AREA_CELL_ORIGIN` = cell 128 at `0x4000`: `0x80` a cell).
pub const CELL_UNITS: i32 = 0x80;
/// The semi-transparency bit of a descriptor (`func_80155560`: `SetSemiTrans(p, (word >> 15) & 1)`).
const SEMI_BIT: u32 = 1 << 15;
/// The semi-transparency mode: bits 12-13, moved to tpage bits 5-6 by `(word >> 7) & 0x60`.
const BLEND_SHIFT: u32 = 12;
/// The grey level: `(word >> 16) & 0xF8`, the same byte for red, green and blue.
const SHADE_SHIFT: u32 = 16;
const SHADE_MASK: u32 = 0xF8;
/// The bit that defers a cell to the depth-sorted list (`func_801530D0`, flag `0x4000`).
const SORTED_BIT: u32 = 1 << 14;
/// The bit that links a cell into its row's second ordering-table slot (`Area_UploadCell` folds it to
/// flag `0x8000`).
const FRONT_BIT: u32 = 1 << 30;

/// The field camera the area init sets and `Area_UpdateCamera` (`exe/slus_004_22@0x801527E4`) applies
/// every frame: `RotMatrix(angle)`, a translation that centres the followed point, and the projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GameCamera {
    /// `g_AreaCameraAngle` as `func_80153DE8` seeds it, in GTE units (4096 a turn). Scripts turn it
    /// later (`Area_TurnCameraX` and its kin), so this is the camera an area opens with.
    pub angle: [i32; 3],
    /// The projection distance `H` (`SetGeomScreen(1000)` in `func_80153DE8`).
    pub focal: i32,
    /// The screen centre (`SetGeomOffset(160, 120)` in `func_80153DE8`).
    pub offset: [i32; 2],
    /// The view distance added to the translation's `z` (`+ 0x1194` in `Area_UpdateCamera`).
    pub distance: i32,
}

/// The camera every area opens with.
pub const GAME_CAMERA: GameCamera =
    GameCamera { angle: [-682, 0, 0x200], focal: 1000, offset: [160, 120], distance: 0x1194 };

/// A model point in the GTE's world space: `x` and `z` move to the GTE's `x` and `y`, and up is the
/// GTE's negative `z` (`func_801530D0`: `vz = -corner * 16`). The cell origin offset is left out:
/// it cancels against the camera's translation.
pub fn to_gte(model: [i32; 3]) -> [i32; 3] {
    [model[0], model[2], -model[1]]
}

/// How a semi-transparent texel mixes with what is behind it: the GPU's four modes (psx-spx, "GPU
/// Texpage Attribute", bits 5-6), `B` the colour behind and `F` the texel.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Blend {
    /// `B / 2 + F / 2`.
    Average,
    /// `B + F`.
    Add,
    /// `B - F`.
    Subtract,
    /// `B + F / 4`.
    AddQuarter,
}

impl Blend {
    /// The mode of tpage bits 5-6.
    pub fn of_mode(mode: u32) -> Self {
        match mode & 3 {
            0 => Self::Average,
            1 => Self::Add,
            2 => Self::Subtract,
            _ => Self::AddQuarter,
        }
    }

    /// The mode number, `0` to `3`.
    pub fn mode(self) -> u8 {
        match self {
            Self::Average => 0,
            Self::Add => 1,
            Self::Subtract => 2,
            Self::AddQuarter => 3,
        }
    }
}

/// What one descriptor word gives a primitive.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Tile {
    /// The descriptor word itself.
    pub descriptor: u32,
    /// Its index in the descriptor table.
    pub index: usize,
    /// The texture page's top-left word in VRAM.
    pub page: [u16; 2],
    pub clut: Clut,
    pub depth: Depth,
    /// The texture coordinates of corners 0 to 3, after the corner-order flags.
    pub uvs: [[u8; 2]; 4],
    /// The grey level the texels are multiplied by; `0x80` leaves them unchanged.
    pub shade: u8,
    /// The semi-transparency mode, when bit 15 is set.
    pub blend: Option<Blend>,
    /// Bit 14: deferred to the depth-sorted list in draw modes 4 and 6.
    pub sorted: bool,
    /// Bit 30: linked into the row's second ordering-table slot.
    pub front: bool,
}

impl Tile {
    /// The tile of descriptor `index` of the bundle, or `None` past the table. A `0` word is drawn
    /// too, as `func_80155560` reads it: block 0 of page bank 0 through CLUT row 483 at grey level 0,
    /// which leaves every texel but the transparent one black.
    pub fn of(bundle: &MapBundle, index: usize) -> Option<Self> {
        let descriptor = *bundle.records.get(index)?;
        let (page_x, page_y, clut, depth) = terrain_descriptor(descriptor).unwrap_or((
            TERRAIN_PAGE_X,
            TERRAIN_PAGE_Y,
            Clut { x: 0, y: TERRAIN_CLUT_ROW },
            Depth::Eight,
        ));
        let (_, uvs) = tile_of(bundle, descriptor);
        Some(Self {
            descriptor,
            index,
            page: [page_x, page_y],
            clut,
            depth,
            uvs,
            shade: ((descriptor >> SHADE_SHIFT) & SHADE_MASK) as u8,
            blend: (descriptor & SEMI_BIT != 0).then(|| Blend::of_mode(descriptor >> BLEND_SHIFT)),
            sorted: descriptor & SORTED_BIT != 0,
            front: descriptor & FRONT_BIT != 0,
        })
    }
}

/// The face of a cell a quad draws.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Face {
    /// The cell's own surface.
    Top,
    /// The wall down from the cell's east edge to its east neighbour.
    East,
    /// The wall down from the cell's south edge to its south neighbour.
    South,
}

impl Face {
    pub fn label(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::East => "east wall",
            Self::South => "south wall",
        }
    }
}

/// One quad of the terrain, in the corner order the primitive takes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quad {
    /// The cell, `row * width + column`.
    pub cell: usize,
    pub face: Face,
    /// The corners `[x, y, z]` in renderer units, in primitive order: the GPU draws the triangles of
    /// corners 0, 1, 2 and 1, 2, 3.
    pub corners: [[i32; 3]; 4],
    /// The tile, `None` when the descriptor index is past the table.
    pub tile: Option<Tile>,
}

/// The quads the field renderer draws for a bundle, cell by cell in row-major order: the top, then
/// the east wall, then the south wall.
pub fn quads(bundle: &MapBundle) -> Vec<Quad> {
    let mut quads = Vec::new();
    for cell in 0..bundle.cells.len() {
        push_cell(bundle, cell, &mut quads);
    }
    quads
}

/// The quads of one cell, in [`quads`] order; empty for a cell the renderer does not draw.
pub fn quads_of_cell(bundle: &MapBundle, cell: usize) -> Vec<Quad> {
    let mut quads = Vec::new();
    push_cell(bundle, cell, &mut quads);
    quads
}

/// The descriptor words a cell's rows take, in [`quads`] order: its top, its east wall and its
/// south wall, as `Area_UploadCell` reads them. Empty for a cell the renderer does not draw.
pub fn descriptor_indices(bundle: &MapBundle, cell: usize) -> Vec<(Face, usize)> {
    quads_of_cell(bundle, cell).into_iter().filter_map(|quad| Some((quad.face, quad.tile?.index))).collect()
}

/// Whether the field renderer draws `cell`: strictly inside the bundle, with a grid value that names
/// a descriptor (`Area_GetCell`, `func_801530D0`).
pub fn is_drawn(bundle: &MapBundle, cell: usize) -> bool {
    let (width, height) = (usize::from(bundle.width), usize::from(bundle.height));
    let (column, row) = (cell % width.max(1), cell / width.max(1));
    (1..width.saturating_sub(1)).contains(&column)
        && (1..height.saturating_sub(1)).contains(&row)
        && descriptor(bundle, cell).is_some()
}

/// Appends the quads of `cell`.
fn push_cell(bundle: &MapBundle, cell: usize, quads: &mut Vec<Quad>) {
    if !is_drawn(bundle, cell) {
        return;
    }
    let width = usize::from(bundle.width);
    let grid = usize::from(bundle.grid[cell]);
    let here = corners(bundle, cell);
    quads.push(Quad { cell, face: Face::Top, corners: here, tile: Tile::of(bundle, grid) });

    let mut next = grid + 1;
    let east = corners(bundle, cell + 1);
    if east[0][1] < here[1][1] || east[2][1] < here[3][1] {
        let corners = [here[3], here[1], east[2], east[0]];
        quads.push(Quad { cell, face: Face::East, corners, tile: Tile::of(bundle, next) });
        next += 1;
    }
    let south = corners(bundle, cell + width);
    if (south[0][1] < here[2][1] || south[1][1] < here[3][1]) && bundle.records.get(next).is_some_and(|&word| word != 0)
    {
        let corners = [here[2], here[3], south[0], south[1]];
        quads.push(Quad { cell, face: Face::South, corners, tile: Tile::of(bundle, next) });
    }
}

/// The four corners of a cell in renderer units, in stored order.
fn corners(bundle: &MapBundle, cell: usize) -> [[i32; 3]; 4] {
    let width = usize::from(bundle.width).max(1);
    let (column, row) = ((cell % width) as i32, (cell / width) as i32);
    let heights = bundle.cells.get(cell).map_or([0; CELL_BYTES], |record| record.heights);
    std::array::from_fn(|corner| {
        let [step_x, step_z] = CORNER_STEPS[corner];
        [(column + step_x) * CELL_UNITS, i32::from(heights[corner]) * HEIGHT_SCALE, (row + step_z) * CELL_UNITS]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::Cell;

    /// A 4 x 4 bundle whose inner 2 x 2 cells have tiles; cell (1, 1) is 2 steps high and its
    /// neighbours flat, so it has an east and a south wall.
    fn bundle() -> MapBundle {
        let mut cells = vec![Cell { heights: [0; 4] }; 16];
        cells[5] = Cell { heights: [2; 4] };
        let mut grid = vec![0; 16];
        grid[5] = 1;
        grid[6] = 4;
        grid[9] = 5;
        grid[10] = 6;
        MapBundle {
            width: 4,
            height: 4,
            cells,
            codes: vec![0; 16],
            grid,
            // Word 0 is never named; 1 is the top, 2 the east wall, 3 the south wall.
            records: vec![0, 0x0080_0000, 0x0081_9001, 0x8180_1000, 0x0080_0002, 0x0080_0003, 0x4080_0004],
            ..MapBundle::default()
        }
    }

    #[test]
    fn a_raised_cell_has_an_east_and_a_south_wall() {
        let quads = quads(&bundle());
        let faces: Vec<(usize, Face)> = quads.iter().map(|quad| (quad.cell, quad.face)).collect();
        assert_eq!(
            faces,
            [(5, Face::Top), (5, Face::East), (5, Face::South), (6, Face::Top), (9, Face::Top), (10, Face::Top)]
        );
        let east = quads[1];
        assert_eq!(east.corners, [[256, 32, 256], [256, 32, 128], [256, 0, 256], [256, 0, 128]]);
        let south = quads[2];
        assert_eq!(south.corners, [[128, 32, 256], [256, 32, 256], [128, 0, 256], [256, 0, 256]]);
        assert_eq!(descriptor_indices(&bundle(), 5), [(Face::Top, 1), (Face::East, 2), (Face::South, 3)]);
    }

    #[test]
    fn a_descriptor_gives_shade_blend_depth_and_order() {
        let bundle = bundle();
        let east = Tile::of(&bundle, 2).expect("a tile");
        assert_eq!(east.shade, 0x80);
        assert_eq!(east.blend, Some(Blend::Add), "bit 15 on, mode 1 in bits 12-13");
        assert_eq!((east.depth, east.clut.x, east.clut.y), (Depth::Eight, 0, 483));
        let south = Tile::of(&bundle, 3).expect("a tile");
        assert_eq!((south.depth, south.clut.x, south.clut.y), (Depth::Four, 16, 483), "bit 31 and CLUT index 1");
        assert_eq!(south.blend, None);
        assert!(Tile::of(&bundle, 6).expect("a tile").front);
        let zero = Tile::of(&bundle, 0).expect("a zero word is a tile");
        assert_eq!((zero.shade, zero.page, zero.depth), (0, [320, 256], Depth::Eight));
        assert!(Tile::of(&bundle, 99).is_none());
    }

    #[test]
    fn a_zero_south_word_frees_the_south_wall_and_the_border_is_not_drawn() {
        let mut bundle = bundle();
        bundle.records[3] = 0;
        assert!(!quads(&bundle).iter().any(|quad| quad.face == Face::South));
        bundle.grid[0] = 1;
        bundle.grid[15] = 1;
        assert!(!quads(&bundle).iter().any(|quad| quad.cell == 0 || quad.cell == 15));
    }
}
