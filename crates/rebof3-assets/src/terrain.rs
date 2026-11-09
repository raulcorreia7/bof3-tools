//! Area terrain as Wavefront OBJ, read from a map bundle.
//!
//! The bundle is `AreaMapBundle` in `decomp/include/world/area.h`, the one C
//! definition of the map header, and its format is
//! `wiki: formats/area#0x80104000-map-bundle`.
//!
//! Each map cell becomes one quad on a unit grid. The mesh builder (`exe/slus_004_22@0x801530D0`)
//! places the cell's corners at the `(x, z)` offsets `(0, 0)`, `(1, 0)`, `(0, 1)` and `(1, 1)`, where
//! `x` is the cell column and `z` the cell row. Corner `k` takes record byte `k`. The export writes
//! `y` as that byte times [`HEIGHT_SCALE`], so a larger byte is higher; the builder stores the same
//! height negated. The OBJ face walks the perimeter (corners 0, 2, 3, 1), because the stored order
//! would cross, and that walk gives the face an upward normal.
//!
//! [`Mesh::heights`] carries the height-byte table of the 13 payloads that ship one. The mesh
//! builder never reads it, so the OBJ does not apply it.
//!
//! Texture coordinates are the tile's, in bytes of a texture page that no field names: the page
//! depends on `GetGraphType()`. The corner-order flags of the tile (bits 16-18) are applied. The
//! material carries no texture, so the MTL file names none.
//!
//! [`top_down`] draws the cells flat with their texels, read from a replayed VRAM through the page
//! and CLUT row of graph type 0 (`rebof3_emi::texture::terrain_descriptor`).

use std::fmt::Write;

use psx_formats::gfx::png;
use rebof3_emi::area::map::{MapBundle, SELECTORS, TileRect};
use rebof3_emi::replay::Vram;
use rebof3_emi::texture::scene::resolve;
use rebof3_emi::texture::terrain_descriptor;

use crate::decode::color_rgba;
use crate::gltf::{self, Primitive};

/// Vertex height per corner-height step (`wiki: formats/area#0x80104000-map-bundle`: each cell byte
/// is scaled by 16).
pub const HEIGHT_SCALE: i32 = 16;
/// The side of the texel block a tile with mode `0` names, in texels.
pub const BLOCK_TEXELS: u16 = 16;
/// The tile-bank selector a tile descriptor names: bits 28-29.
const SELECTOR_SHIFT: u32 = 28;
/// The tile mode: bits 8-11, `0` for the 16x16 block.
const MODE_SHIFT: u32 = 8;
/// The bit that picks the quad bank over the rectangle bank.
const QUAD_BIT: u32 = 1 << 11;
/// The corner-order flags. Bit 18 turns the corners a quarter, bit 16 flips them across `x` and bit
/// 17 across `z` (`@0x80155560`, `:119-157`).
const TURN_BIT: u32 = 1 << 18;
const FLIP_X_BIT: u32 = 1 << 16;
const FLIP_Z_BIT: u32 = 1 << 17;
/// The `(x, z)` offset of each corner from the cell's near corner, in stored order.
pub(crate) const CORNER_STEPS: [[i32; 2]; 4] = [[0, 0], [1, 0], [0, 1], [1, 1]];
/// The corners in the order an OBJ face walks them: around the perimeter, not in stored order. The
/// walk runs counter-clockwise seen from above, so the face normal points up.
const PERIMETER: [usize; 4] = [0, 2, 3, 1];
/// Texture coordinates are bytes; the OBJ writes them as fractions of this span.
const COORDINATE_SPAN: f32 = 256.0;

/// The OBJ and MTL text of one map bundle.
pub struct Terrain {
    /// The mesh: one quad per cell, grouped by material.
    pub obj: String,
    /// One material per tile-bank selector, with no texture.
    pub mtl: String,
}

/// The terrain of one map bundle: four corners per cell, in the stored corner order.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Mesh {
    /// Cell columns. The cell at `(column, row)` is number `row * columns + column`.
    pub columns: usize,
    /// Cell rows.
    pub rows: usize,
    /// Corner positions `[x, y, z]`, four per cell, in stored order: `x` is the column plus the
    /// corner's step, `z` the row plus its step, and `y` the corner byte times [`HEIGHT_SCALE`].
    pub positions: Vec<[i32; 3]>,
    /// Texture coordinates per corner, in bytes of a texture page that no field names.
    pub uvs: Vec<[u16; 2]>,
    /// The tile-bank selector of each cell: the material it is drawn with.
    pub selectors: Vec<u8>,
    /// The height-byte table, one signed byte per cell, when the payload ships it. The height query
    /// reads it; the mesh does not.
    pub heights: Option<Vec<i8>>,
}

/// The terrain of a map bundle payload.
pub fn mesh_from_payload(payload: &[u8]) -> rebof3_emi::Result<Mesh> {
    Ok(mesh_of_bundle(&MapBundle::decode(payload)?))
}

/// The terrain of a decoded map bundle.
pub fn mesh_of_bundle(bundle: &MapBundle) -> Mesh {
    let width = usize::from(bundle.width).max(1);
    let mut mesh = Mesh {
        columns: usize::from(bundle.width),
        rows: usize::from(bundle.height),
        heights: bundle.heights.as_ref().map(|table| table.iter().map(|&byte| byte as i8).collect()),
        ..Mesh::default()
    };
    for cell in 0..bundle.cells.len() {
        let (column, row) = ((cell % width) as i32, (cell / width) as i32);
        let heights = bundle.cells[cell].heights.map(|height| i32::from(height) * HEIGHT_SCALE);
        for (corner, [step_x, step_z]) in CORNER_STEPS.iter().enumerate() {
            mesh.positions.push([column + step_x, heights[corner], row + step_z]);
        }
        let (selector, uvs) = tile(bundle, cell);
        mesh.uvs.extend(uvs.map(|[u, v]| [u16::from(u), u16::from(v)]));
        mesh.selectors.push(selector as u8);
    }
    mesh
}

/// The OBJ and MTL text of a mesh. `mtl_name` is the MTL file the OBJ names.
pub fn of_mesh(mesh: &Mesh, mtl_name: &str) -> Terrain {
    let mut obj = String::new();
    writeln!(obj, "# One quad per map cell: x = column, z = row, y = corner byte x {HEIGHT_SCALE}.")
        .expect("a String never fails");
    writeln!(obj, "mtllib {mtl_name}").expect("a String never fails");
    let cells = mesh.selectors.len().min(mesh.positions.len() / 4).min(mesh.uvs.len() / 4);
    let mut current = None;
    for cell in 0..cells {
        let selector = mesh.selectors[cell];
        if current != Some(selector) {
            writeln!(obj, "usemtl bank{selector}").expect("a String never fails");
            current = Some(selector);
        }
        for [x, y, z] in &mesh.positions[4 * cell..4 * cell + 4] {
            writeln!(obj, "v {x} {y} {z}").expect("a String never fails");
        }
        for [u, v] in &mesh.uvs[4 * cell..4 * cell + 4] {
            let (u, v) = (f32::from(*u) / COORDINATE_SPAN, f32::from(*v) / COORDINATE_SPAN);
            writeln!(obj, "vt {u} {v}").expect("a String never fails");
        }
        let face = PERIMETER.map(|corner| 4 * cell + corner + 1);
        writeln!(obj, "f {0}/{0} {1}/{1} {2}/{2} {3}/{3}", face[0], face[1], face[2], face[3])
            .expect("a String never fails");
    }
    let mut mtl =
        String::from("# Materials carry no texture: a tile's page and palette are not named by the payload.\n");
    for selector in 0..SELECTORS {
        writeln!(mtl, "newmtl bank{selector}\nKd 1.0 1.0 1.0\n").expect("a String never fails");
    }
    Terrain { obj, mtl }
}

/// Builds the OBJ and MTL text of a decoded map bundle.
pub fn of_bundle(bundle: &MapBundle, mtl_name: &str) -> Terrain {
    of_mesh(&mesh_of_bundle(bundle), mtl_name)
}

/// Builds the OBJ and MTL text of a map bundle payload. `mtl_name` is the MTL file the OBJ names.
pub fn from_payload(payload: &[u8], mtl_name: &str) -> rebof3_emi::Result<Terrain> {
    Ok(of_mesh(&mesh_from_payload(payload)?, mtl_name))
}

/// The layout of a map bundle as JSON: the byte code of each cell, one row per map row; the object list,
/// each object with its tag and body words; and the height-byte table when the payload ships one. The
/// byte codes and words keep their stored values. Their meanings are not decoded
/// (`wiki: formats/area#0x80104000-map-bundle`, "Map bundle"). The field names follow the C record
/// (`AreaMapBundle` in
/// `decomp/include/world/area.h`).
pub fn layout_json(payload: &[u8]) -> rebof3_emi::Result<Vec<u8>> {
    let bundle = MapBundle::decode(payload)?;
    let width = usize::from(bundle.width).max(1);
    let cells = usize::from(bundle.width) * usize::from(bundle.height);
    let codes = &bundle.codes[..cells.min(bundle.codes.len())];
    let rows: Vec<&[u8]> = codes.chunks(width).collect();
    let objects: Vec<serde_json::Value> =
        bundle.objects.iter().map(|object| serde_json::json!({ "tag": object.tag, "words": object.words })).collect();
    let document = serde_json::json!({
        "width": bundle.width,
        "height": bundle.height,
        "byte_codes": rows,
        "objects": objects,
        "heights": bundle.heights,
    });
    Ok(serde_json::to_vec_pretty(&document).expect("a JSON value always serializes"))
}

/// The terrain as a binary glTF (`.glb`), with one primitive per tile-bank material and the same
/// corners as the OBJ. Each quad is two triangles that face up: corners 0, 2, 3 and 0, 3, 1. Returns
/// `None` when the mesh has no cell.
pub fn glb_of_mesh(mesh: &Mesh, name: &str) -> Option<Vec<u8>> {
    let positions: Vec<[f32; 3]> = mesh.positions.iter().map(|[x, y, z]| [*x as f32, *y as f32, *z as f32]).collect();
    let uvs: Vec<[f32; 2]> =
        mesh.uvs.iter().map(|[u, v]| [f32::from(*u) / COORDINATE_SPAN, f32::from(*v) / COORDINATE_SPAN]).collect();
    let cells = mesh.selectors.len().min(positions.len() / 4).min(uvs.len() / 4);
    let mut primitives: Vec<Primitive> =
        (0..SELECTORS).map(|material| Primitive { material, indices: Vec::new() }).collect();
    for cell in 0..cells {
        let base = (4 * cell) as u32;
        primitives[usize::from(mesh.selectors[cell])].indices.extend([
            base,
            base + 2,
            base + 3,
            base,
            base + 3,
            base + 1,
        ]);
    }
    primitives.retain(|primitive| !primitive.indices.is_empty());
    let materials: Vec<String> = (0..SELECTORS).map(|selector| format!("bank{selector}")).collect();
    let names: Vec<&str> = materials.iter().map(String::as_str).collect();
    gltf::glb(name, &positions, &uvs, &primitives, &names)
}

/// The tile-bank selector and the four texture-coordinate corners of a cell, in corner order.
///
/// A cell with grid value `0` is empty and has the zero tile. Otherwise the grid value indexes the
/// descriptor words. Mode `0` names a 16x16 block from the descriptor's low byte. Otherwise bit 11
/// picks the rectangle bank (index `descriptor & 0xFF`) or the quad bank (index `descriptor & 0x7FF`).
fn tile(bundle: &MapBundle, cell: usize) -> (usize, [[u8; 2]; 4]) {
    let Some(descriptor) = descriptor(bundle, cell) else {
        return (0, [[0; 2]; 4]);
    };
    let selector = ((descriptor >> SELECTOR_SHIFT) & 3) as usize;
    let mode = (descriptor >> MODE_SHIFT) & 0xf;
    let uvs = if mode == 0 {
        let block = BLOCK_TEXELS as u8;
        let u = (descriptor as u8 & 0xf) * block;
        let v = ((descriptor >> 4) as u8 & 0xf) * block;
        rectangle_corners(u, v, block - 1, block - 1)
    } else if descriptor & QUAD_BIT == 0 {
        bundle.rects[selector]
            .get((descriptor & 0xff) as usize)
            .map_or([[0; 2]; 4], |rect: &TileRect| rectangle_corners(rect.u, rect.v, rect.du, rect.dv))
    } else {
        bundle.quads[selector].get((descriptor & 0x7ff) as usize).map_or([[0; 2]; 4], |quad| quad.uvs)
    };
    (selector, corner_order(descriptor, uvs))
}

/// The tile descriptor of a cell: `None` for an empty cell (grid value `0`) or a grid value past the
/// descriptor table.
fn descriptor(bundle: &MapBundle, cell: usize) -> Option<u32> {
    let value = usize::from(bundle.grid.get(cell).copied().unwrap_or(0));
    bundle.records.get(value).filter(|_| value != 0).copied()
}

/// The corners of a rectangle in corner order: its near corner, then one extent along `u`, then
/// along `v`, then both. Coordinates wrap at 256, as the GPU's byte coordinates do.
fn rectangle_corners(u: u8, v: u8, du: u8, dv: u8) -> [[u8; 2]; 4] {
    [[u, v], [u.wrapping_add(du), v], [u, v.wrapping_add(dv)], [u.wrapping_add(du), v.wrapping_add(dv)]]
}

/// Applies a tile's corner-order flags to its four corners, in the order `@0x80155560` applies them:
/// the quarter turn (bit 18), the flip across `x` (bit 16), then the flip across `z` (bit 17).
fn corner_order(descriptor: u32, uvs: [[u8; 2]; 4]) -> [[u8; 2]; 4] {
    let mut uvs = uvs;
    if descriptor & TURN_BIT != 0 {
        uvs = [uvs[2], uvs[0], uvs[3], uvs[1]];
    }
    if descriptor & FLIP_X_BIT != 0 {
        uvs = [uvs[1], uvs[0], uvs[3], uvs[2]];
    }
    if descriptor & FLIP_Z_BIT != 0 {
        uvs = [uvs[2], uvs[3], uvs[0], uvs[1]];
    }
    uvs
}

/// Pixels per side of a cell in a [`TopDown`] view: one 16x16 block of texels.
pub const CELL_PIXELS: usize = BLOCK_TEXELS as usize;

/// A map bundle seen from above with its tiles' texels: each cell is a [`CELL_PIXELS`] square, row
/// `r` and column `c` at `(16c, 16r)`. A cell's texels come from VRAM through its descriptor's page
/// and CLUT (the terrain rule, [`terrain_descriptor`]) and its four texture corners.
///
/// The square is drawn as the GPU draws the cell's four-corner primitive seen flat: two triangles,
/// corners 0, 1, 2 and 1, 2, 3, each with affine texture coordinates. Corner `k` sits at the cell's
/// step `(0, 0)`, `(1, 0)`, `(0, 1)` or `(1, 1)` (the mesh builder `@0x801530D0`). The vertex colour
/// in descriptor bits 19-23 and the semi-transparency bit 15 are not applied, and neither is the
/// height: the view is flat.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TopDown {
    pub width: usize,
    pub height: usize,
    /// One colour word per pixel, row-major; `None` where the cell has no tile.
    pub words: Vec<Option<u16>>,
    /// Cells whose page an upload wrote and whose CLUT row one entry wrote.
    pub textured: usize,
    /// Cells with a tile whose page or CLUT row the VRAM does not hold.
    pub unwritten: usize,
}

impl TopDown {
    /// The RGBA view of every pixel; a cell with no tile, and the transparent colour `0x0000`, are
    /// clear.
    pub fn rgba(&self) -> Vec<[u8; 4]> {
        self.words.iter().map(|word| word.map_or([0; 4], color_rgba)).collect()
    }

    /// The view as an RGBA PNG.
    pub fn png(&self) -> Vec<u8> {
        let bytes: Vec<u8> = self.rgba().into_iter().flatten().collect();
        png::rgba_png(self.width as u32, self.height as u32, &bytes)
    }
}

/// The bundle seen from above, its texels read from `vram`.
pub fn top_down(bundle: &MapBundle, vram: &Vram) -> TopDown {
    let (columns, rows) = (usize::from(bundle.width), usize::from(bundle.height));
    let width = columns * CELL_PIXELS;
    let mut view = TopDown {
        width,
        height: rows * CELL_PIXELS,
        words: vec![None; width * rows * CELL_PIXELS],
        textured: 0,
        unwritten: 0,
    };
    for cell in 0..(columns * rows).min(bundle.grid.len()) {
        let Some(word) = descriptor(bundle, cell) else { continue };
        let Some((page_x, page_y, clut, depth)) = terrain_descriptor(word) else { continue };
        if resolve(vram, page_x, page_y, clut, depth).is_some() {
            view.textured += 1;
        } else {
            view.unwritten += 1;
        }
        let (_, corners) = tile(bundle, cell);
        let (left, top) = ((cell % columns) * CELL_PIXELS, (cell / columns) * CELL_PIXELS);
        for y in 0..CELL_PIXELS {
            for x in 0..CELL_PIXELS {
                let [u, v] = texel_at(corners, x, y);
                view.words[(top + y) * width + left + x] = Some(vram.fetch(page_x, page_y, clut, depth, u, v));
            }
        }
    }
    view
}

/// The texture coordinate at pixel `x`, `y` of a cell: affine across the triangle of corners 0, 1,
/// 2 when `x + y` is at most the cell's last pixel, else across corners 3, 2, 1.
fn texel_at(corners: [[u8; 2]; 4], x: usize, y: usize) -> [u8; 2] {
    let last = (CELL_PIXELS - 1) as f32;
    let (s, t) = (x as f32 / last, y as f32 / last);
    let at = |corner: usize, axis: usize| f32::from(corners[corner][axis]);
    let (origin, along_s, along_t, s, t) = if s + t <= 1.0 { (0, 1, 2, s, t) } else { (3, 2, 1, 1.0 - s, 1.0 - t) };
    [0, 1].map(|axis| {
        let value =
            at(origin, axis) + s * (at(along_s, axis) - at(origin, axis)) + t * (at(along_t, axis) - at(origin, axis));
        value.round().clamp(0.0, 255.0) as u8
    })
}

#[cfg(test)]
mod top_down_tests {
    use super::*;
    use rebof3_emi::Entry;
    use rebof3_emi::area::map::Cell;
    use rebof3_emi::replay::Writer;
    use rebof3_emi::texture::{TERRAIN_CLUT_ROW, TERRAIN_PAGE_X, TERRAIN_PAGE_Y};

    /// A VRAM whose terrain page 0 holds one 32x32-word upload with the 8-bit texel `u + 16 * v`
    /// at `(u, v)`, and whose first CLUT row maps index `i` to colour `0x100 + i`.
    fn vram() -> Vram {
        let texel = |u: usize, v: usize| ((u + 16 * v) & 0xFF) as u16;
        let payload: Vec<u8> = (0..32 * 32)
            .flat_map(|at| {
                let (u, v) = (2 * (at % 32), at / 32);
                (texel(u, v) | (texel(u + 1, v) << 8)).to_le_bytes()
            })
            .collect();
        // The load argument packs the rectangle column and row (in 32-word steps) and one chunk.
        let at = (u32::from(TERRAIN_PAGE_X / 32) << 24) | (u32::from(TERRAIN_PAGE_Y / 32) << 16) | (1 << 8);
        let entry = Entry { size: payload.len() as u32, offset: 0, ram_ptr: at, first4: 0, file_type: 3 };
        let mut vram = Vram::new();
        vram.load("A", &[entry], &[&payload]);
        let colors: Vec<u16> = (0..256).map(|index| 0x100 + index).collect();
        vram.load_image(0, usize::from(TERRAIN_CLUT_ROW), 256, 1, &colors, Writer::entry("A", 2));
        vram
    }

    fn bundle(descriptor: u32) -> MapBundle {
        MapBundle {
            width: 2,
            height: 1,
            cells: vec![Cell { heights: [0; 4] }; 2],
            codes: vec![0; 4],
            grid: vec![1, 0],
            records: vec![0, descriptor],
            ..MapBundle::default()
        }
    }

    #[test]
    fn a_block_tile_copies_its_sixteen_texels() {
        // Mode 0, block 0: texels (0..16, 0..16) of page 0, 8-bit, CLUT row 483, vertex colour 0x80
        // (descriptor 0 is the empty tile).
        let view = top_down(&bundle(0x0080_0000), &vram());
        assert_eq!((view.width, view.height, view.textured, view.unwritten), (32, 16, 1, 0));
        for (x, y) in [(0, 0), (15, 0), (0, 15), (7, 9), (15, 15)] {
            assert_eq!(view.words[y * 32 + x], Some(0x100 + (x + 16 * y) as u16), "pixel ({x}, {y})");
        }
        assert_eq!(view.words[16], None, "the second cell has no tile");
    }

    #[test]
    fn a_flipped_tile_mirrors_its_texels() {
        // Bit 16 flips the corners across x.
        let view = top_down(&bundle(0x0081_0000), &vram());
        assert_eq!(view.words[0], Some(0x100 + 15));
        assert_eq!(view.words[15], Some(0x100));
    }

    #[test]
    fn a_tile_whose_page_nothing_wrote_counts_as_unwritten() {
        let view = top_down(&bundle(0x1080_0000), &vram());
        assert_eq!((view.textured, view.unwritten), (0, 1));
        assert_eq!(&view.png()[1..4], b"PNG");
    }
}
