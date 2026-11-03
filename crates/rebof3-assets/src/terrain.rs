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

use std::fmt::Write;

use rebof3_emi::area::map::{MapBundle, SELECTORS, TileRect};

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
    let value = usize::from(bundle.grid.get(cell).copied().unwrap_or(0));
    let Some(&descriptor) = bundle.records.get(value).filter(|_| value != 0) else {
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
