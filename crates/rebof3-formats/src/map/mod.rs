//! Area map bundle codec (`0x80104000`).
//!
//! [`terrain`] exports the bundle as OBJ and glTF and imports edited heights; [`layout`] exports and
//! imports the byte codes, objects and height table as JSON.
//!
//! The WORLD* archives' map payload: a 0x30-byte header of section offsets in
//! 4-byte units, the per-cell tables, the tile records and four record regions.
//! [`MapBundle::decode`] splits a payload into its sections and
//! [`MapBundle::encode`] lays them out again, deriving every header offset and
//! every size field from the section contents, so a decode → encode cycle is
//! byte-exact and a section can be edited without touching the header.
//!
//! Region order, proven against all 200 payloads (`wiki: formats/area`
//! records the readers behind every access):
//!
//! ```text
//! +0x00 u8  width                 +0x01 u8  height
//! +0x02 u16 grid_off4     = 12 + W*H + align4(W*H)/4
//! +0x04 u16 rect_off4[4]  rectangle table: 4-byte tile rectangles, stride 4 bytes,
//!                                  selected by (descriptor >> 28) & 3
//! +0x06 u16 quad_off4[4]  quad table: 8-byte tile quads, stride 8 bytes, same
//!                                  selector; the tables interleave in memory as
//!                                  R[i], Q[i], R[i+1], Q[i+1], ...
//! +0x14 u16 code_off4     = 12 + W*H: per-cell byte codes, one byte per cell,
//!                                  row stride `width`
//! +0x1a u16 colour[2]     runtime colour words (RGB555), index (mask >> 3)
//! +0x1e u16 scroll_seat   scroll seat; the area init seeds D_80149338 with it
//! +0x20 u16 list_a_off4   record list, opcode byte & 0x3f
//! +0x22 u16 list_b_off4   record list, opcode byte & 0x7f
//! +0x24 u16 cell_off4     record cells, ids held in D_80141880
//! +0x28 u16 object_off4   object list, tag + body size
//! +0x2a u16 height_off4   height bytes, one per cell, row stride `width`
//! ```
//!
//! Each of the four record regions ends with a zero word: the walks in
//! `Area_RunBundleOpcodes` (both loops), `Area_CyclePalette`, `Area_IndexCells`,
//! `Area_SetCellPalette` and `Area_DrawCellLayer` stop on it. Every region's records tile
//! exactly onto the next region's start, which is why [`MapBundle::encode`]
//! derives the offsets from the sections rather than storing them.
//!
//! `height_off4` equals the payload end in 187 payloads — the engine builds the
//! height table in RAM right after the bundle — and holds the last `W*H` bytes
//! in the other 13, so [`MapBundle::heights`] is `Some` exactly there.

use rebof3_emi::Entry;

use crate::error::{Error, Result};
use crate::identity::difference;
use crate::registry::{Description, Extra, Extras, Format, RoundTrip, Selector, Site, State};

pub mod codes;
pub mod draw;
pub mod layout;
pub mod terrain;
use psx_formats::bytes::{u16_le, u32_le};

/// The load address of the bundle.
const LOAD_ADDRESS: u32 = 0x8010_4000;

/// The bundle as a format-registry row.
pub static BUNDLE: Description = Description {
    format: Format::AreaMapBundle,
    name: "area map bundle (0x80104000)",
    state: State::Codec,
    spec_state: "documented",
    codec_state: "decode+encode",
    evidence: "wiki: formats/area (`0x80104000`)",
    extension: "map",
    selector: Selector::Load(&[LOAD_ADDRESS]),
    round_trip: RoundTrip::Payload(round_trip),
    derive: None,
    open: Some(open),
    kind: "area-map",
};

/// The layout the terrain is read from, cited in the catalog's evidence column.
const EVIDENCE: &str = "wiki: formats/area#0x80104000-map-bundle (tile and cell layout)";

/// The terrain as OBJ, MTL and glTF binary, and the layout as JSON.
fn open(site: &Site<'_>) -> Extras {
    let (payload, slot) = (site.payload(), site.slot);
    let mut extras = Extras::default();
    let mut push = |role: &'static str, tag: &str, extension: &'static str, data: Vec<u8>| {
        extras.files.push(Extra { role, tag: tag.to_string(), extension, evidence: EVIDENCE.to_string(), data });
    };
    let mut skipped = None;
    match MapBundle::decode(payload) {
        Ok(bundle) => {
            let text = terrain::of_bundle(&bundle, &format!("{slot}.mtl"));
            push("obj", "", "obj", text.obj.into_bytes());
            push("mtl", "", "mtl", text.mtl.into_bytes());
            if let Some(glb) = terrain::glb_of_bundle(&bundle, &slot.to_string(), &[terrain::cells_node(&bundle)]) {
                push("glb", "", "glb", glb);
            }
        }
        Err(error) => skipped = Some(format!("terrain skipped: {error}")),
    }
    match layout::export(payload) {
        Ok(data) => push("json", ".layout", "json", data),
        Err(error) => skipped = Some(format!("layout skipped: {error}")),
    }
    extras.skipped = skipped;
    extras
}

/// Decodes the bundle and encodes it again; `Err` names the first difference.
fn round_trip(_entry: &Entry, payload: &[u8]) -> Result<(), String> {
    let bundle = MapBundle::decode(payload).map_err(|error| error.to_string())?;
    difference(&bundle.encode(), payload)
}

/// The header, in bytes.
pub const HEADER_BYTES: usize = 0x30;
/// One per-cell record: four signed corner heights.
pub const CELL_BYTES: usize = 4;
/// One cell-grid value.
pub const GRID_BYTES: usize = 2;
/// One grid-indexed tile descriptor word.
pub const TILE_BYTES: usize = 4;
/// One rectangle-table tile rectangle.
pub const RECT_BYTES: usize = 4;
/// One quad-table tile quad.
pub const QUAD_BYTES: usize = 8;
/// Texture pages a bundle selects between, by `(descriptor >> 28) & 3`.
pub const SELECTORS: usize = 4;

/// One per-cell record: the four signed corner heights of the cell. Every
/// reader loads all four as `s8` and scales by 16 for the vertex height
/// (`@0x801530D0`, `@0x8015684C`, `@0x80157CE4`); the height query adds the
/// height-table byte to the corners (`@0x8015477C`). No 16-bit field exists.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Cell {
    /// Corner heights, in record byte order.
    pub heights: [i8; CELL_BYTES],
}

/// One rectangle-table tile record: the texture rectangle the renderer expands into the
/// quad's four UV pairs. Byte order is `dv`, `du`, `v`, `u`: the renderer takes
/// byte 3 as `u` and byte 2 as `v`, and adds byte 1 to `u` and byte 0 to `v`
/// for the far corner (`@0x80155560`, the four-byte rectangle path).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TileRect {
    pub u: u8,
    pub v: u8,
    pub du: u8,
    pub dv: u8,
}

/// One quad-table tile record: four independent UV pairs, one per quad corner. Each
/// pair is stored as `(v, u)` bytes, and corner `k` reads the pair at bytes
/// `(3, 2)`, `(1, 0)`, `(7, 6)` and `(5, 4)` for `k` = 0 to 3, before the
/// corner-order flags apply (`exe/slus_004_22@0x80155560`, loads at
/// `0x801555E0` to `0x80155644`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TileQuad {
    /// Corner texture coordinates `[u, v]`, in corner order.
    pub uvs: [[u8; 2]; 4],
}

/// One record of a `+0x20`, `+0x22` or `+0x24` body: a header word whose byte 3
/// is the dispatcher's opcode and whose byte 2 is the record's size in 4-byte
/// words, counting the header, then the operand words (`size - 1` of them).
/// Bytes 0-1 are operands the dispatchers read.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ListRecord {
    /// Byte 3 of the header word; the masks differ per region.
    pub opcode: u8,
    /// Bytes 0 and 1 of the header word, in order.
    pub head: [u8; 2],
    /// The words after the header.
    pub operands: Vec<u32>,
}

impl ListRecord {
    /// The record's size in 4-byte words, header included.
    pub fn words(&self) -> usize {
        1 + self.operands.len()
    }

    /// The header word and operands, as they appear in the payload.
    fn write(&self, out: &mut Vec<u8>) {
        let head = (u32::from(self.opcode) << 24) | (self.words() as u32 & 0xff) << 16 | u32::from(self.head[1]) << 8 | u32::from(self.head[0]);
        out.extend_from_slice(&head.to_le_bytes());
        for operand in &self.operands {
            out.extend_from_slice(&operand.to_le_bytes());
        }
    }
}

/// One `+0x24` record cell: a head of map coordinates and its record chain.
/// The runner writes this cell's id into the `D_80141880` grid after the
/// isometric transform (`Area_IndexCells`), and the two walkers advance through
/// the chain by each record's size byte (`Area_SetCellPalette`, `Area_DrawCellLayer`).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MapCell {
    /// Head byte 0: map row.
    pub y: u8,
    /// Head byte 1: map column.
    pub x: u8,
    /// The cell's records, tiling the body exactly.
    pub records: Vec<ListRecord>,
}

/// One `+0x28` object: a `u16` tag then the body's words, whose count the
/// reader takes from the head (`func_801576FC` walks one object's body,
/// `Area_RunBundleOpcodes`'s second loop walks the objects).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MapObject {
    /// The tag `func_801576FC` passes to `Area_TestCondition`.
    pub tag: u16,
    /// The body words after the head.
    pub words: Vec<u32>,
}

/// One map bundle payload's sections.
///
/// The per-cell vectors hold `width * height` entries: `cells` records, `codes`
/// bytes and `grid` values. A list record holds at most 255 words and a cell at
/// most 65,535, the ranges their size fields carry.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MapBundle {
    pub width: u8,
    pub height: u8,
    /// Runtime colour words, indexed `(mask >> 3)` by `@0x80156ED4`.
    pub colour: [u16; 2],
    /// Scroll seat, read by the area init and the scroll step.
    pub scroll_seat: u16,
    /// `width * height` per-cell records, row stride `width`.
    pub cells: Vec<Cell>,
    /// The per-cell byte-code section (`D_8014931C`), `align4(width * height)`
    /// bytes: one code per cell, then up to three pad bytes.
    pub codes: Vec<u8>,
    /// `width * height` grid cells; `0` is empty.
    pub grid: Vec<u16>,
    /// The grid-indexed tile descriptor words, addressed `grid_end + cell * 4`.
    pub records: Vec<u32>,
    /// Rectangle-table tile rectangles, one table per selector.
    pub rects: [Vec<TileRect>; SELECTORS],
    /// Quad-table tile quads, one table per selector.
    pub quads: [Vec<TileQuad>; SELECTORS],
    /// The `+0x20` record list, opcode byte `& 0x3f`.
    pub list_a: Vec<ListRecord>,
    /// The `+0x22` record list, opcode byte `& 0x7f`.
    pub list_b: Vec<ListRecord>,
    /// The `+0x28` object list.
    pub objects: Vec<MapObject>,
    /// The `+0x24` record cells, ids held in `D_80141880`.
    pub cells_24: Vec<MapCell>,
    /// The height-byte table, when the payload ships it (13 payloads).
    pub heights: Option<Vec<u8>>,
    /// The five header words that are zero in all 200 payloads and read by
    /// nothing (`+0x16`, `+0x18`, `+0x26`, `+0x2c`, `+0x2e`), carried so a
    /// decode → encode cycle never drops bytes.
    pub reserved: [u16; 5],
}

/// Every section's byte range, as the header fields give it.
struct Layout {
    codes: usize,
    grid: usize,
    grid_end: usize,
    records_end: usize,
    rects: [usize; SELECTORS],
    quads: [usize; SELECTORS],
    list_a: usize,
    list_b: usize,
    objects: usize,
    cells_24: usize,
    end: usize,
}

impl MapBundle {
    /// Reads a map bundle payload.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        let width = *bytes.first().ok_or(Error::MapBundleTooShort(0))?;
        let height = *bytes.get(1).ok_or(Error::MapBundleTooShort(1))?;
        let layout = Layout::read(bytes)?;
        let cells = width as usize * height as usize;

        let mut corners = [0; CELL_BYTES];
        let mut per_cell = Vec::with_capacity(cells);
        for index in 0..cells {
            let at = HEADER_BYTES + index * CELL_BYTES;
            for (corner, height) in corners.iter_mut().enumerate() {
                *height = bytes[at + corner] as i8;
            }
            per_cell.push(Cell { heights: corners });
        }
        let codes = bytes[layout.codes..layout.grid].to_vec();
        let grid = (0..cells).map(|index| u16_le(bytes, layout.grid + index * GRID_BYTES)).collect();
        let records = (0..(layout.records_end - layout.grid_end) / TILE_BYTES).map(|index| u32_le(bytes, layout.grid_end + index * TILE_BYTES)).collect();

        let mut rects: [Vec<TileRect>; SELECTORS] = Default::default();
        let mut quads: [Vec<TileQuad>; SELECTORS] = Default::default();
        for selector in 0..SELECTORS {
            let start = layout.rects[selector];
            rects[selector] = (start..layout.quads[selector]).step_by(RECT_BYTES).map(|at| TileRect { u: bytes[at + 3], v: bytes[at + 2], du: bytes[at + 1], dv: bytes[at] }).collect();
            let start = layout.quads[selector];
            let stop = layout.rects.get(selector + 1).copied().unwrap_or(layout.list_a);
            quads[selector] = (start..stop)
                .step_by(QUAD_BYTES)
                .map(|at| TileQuad { uvs: [[bytes[at + 3], bytes[at + 2]], [bytes[at + 1], bytes[at]], [bytes[at + 7], bytes[at + 6]], [bytes[at + 5], bytes[at + 4]]] })
                .collect();
        }

        Ok(Self {
            width,
            height,
            colour: [u16_le(bytes, 0x1a), u16_le(bytes, 0x1c)],
            scroll_seat: u16_le(bytes, 0x1e),
            cells: per_cell,
            codes,
            grid,
            records,
            rects,
            quads,
            list_a: read_list(bytes, layout.list_a, layout.list_b)?,
            list_b: read_list(bytes, layout.list_b, layout.objects)?,
            objects: read_objects(bytes, layout.objects, layout.cells_24)?,
            cells_24: read_cells(bytes, layout.cells_24, layout.end)?,
            heights: (layout.end < bytes.len()).then(|| bytes[layout.end..].to_vec()),
            reserved: [u16_le(bytes, 0x16), u16_le(bytes, 0x18), u16_le(bytes, 0x26), u16_le(bytes, 0x2c), u16_le(bytes, 0x2e)],
        })
    }

    /// The payload bytes, with every header offset and size field derived from
    /// the sections.
    pub fn encode(&self) -> Vec<u8> {
        let cells = self.width as usize * self.height as usize;
        let grid_off4 = 12 + cells + cells.div_ceil(4);
        let mut offset = grid_off4 + cells.div_ceil(2) + self.records.len() * (TILE_BYTES / 4);
        let mut rect_off4 = [0; SELECTORS];
        let mut quad_off4 = [0; SELECTORS];
        for selector in 0..SELECTORS {
            rect_off4[selector] = offset;
            offset += self.rects[selector].len();
            quad_off4[selector] = offset;
            offset += self.quads[selector].len() * (QUAD_BYTES / RECT_BYTES);
        }
        let list_a_off4 = offset;
        offset += list_words(&self.list_a) + 1;
        let list_b_off4 = offset;
        offset += list_words(&self.list_b) + 1;
        let object_off4 = offset;
        offset += self.objects.iter().map(object_words).sum::<usize>() + 1;
        let cell_off4 = offset;
        offset += self.cells_24.iter().map(cell_words).sum::<usize>() + 1;

        // The header fields at +0x02 and up, in offset order: one u16 each.
        let mut header = [0u16; (HEADER_BYTES - 2) / GRID_BYTES];
        header[0] = grid_off4 as u16;
        for selector in 0..SELECTORS {
            header[1 + 2 * selector] = rect_off4[selector] as u16;
            header[2 + 2 * selector] = quad_off4[selector] as u16;
        }
        header[9] = (12 + cells) as u16;
        header[10] = self.reserved[0];
        header[11] = self.reserved[1];
        header[12] = self.colour[0];
        header[13] = self.colour[1];
        header[14] = self.scroll_seat;
        header[15] = list_a_off4 as u16;
        header[16] = list_b_off4 as u16;
        header[17] = cell_off4 as u16;
        header[18] = self.reserved[2];
        header[19] = object_off4 as u16;
        header[20] = offset as u16;
        header[21] = self.reserved[3];
        header[22] = self.reserved[4];

        let mut out = Vec::with_capacity(self.encoded_len());
        out.push(self.width);
        out.push(self.height);
        for value in header {
            out.extend_from_slice(&value.to_le_bytes());
        }
        for cell in &self.cells {
            out.extend(cell.heights.iter().map(|height| *height as u8));
        }
        out.extend_from_slice(&self.codes);
        for value in &self.grid {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.resize(HEADER_BYTES + cells * CELL_BYTES + align4(cells) + align4(cells * GRID_BYTES), 0);
        for record in &self.records {
            out.extend_from_slice(&record.to_le_bytes());
        }
        for selector in 0..SELECTORS {
            for rect in &self.rects[selector] {
                out.extend_from_slice(&[rect.dv, rect.du, rect.v, rect.u]);
            }
            for quad in &self.quads[selector] {
                for corner in [1, 0, 3, 2] {
                    out.push(quad.uvs[corner][1]);
                    out.push(quad.uvs[corner][0]);
                }
            }
        }
        write_list(&mut out, &self.list_a);
        write_list(&mut out, &self.list_b);
        for object in &self.objects {
            let head = u32::from(object.tag) | (object.words.len() as u32) << 16;
            out.extend_from_slice(&head.to_le_bytes());
            for word in &object.words {
                out.extend_from_slice(&word.to_le_bytes());
            }
        }
        out.extend_from_slice(&[0; 4]);
        for cell in &self.cells_24 {
            let head = u32::from(cell.y) | u32::from(cell.x) << 8 | (cell_words(cell) as u32) << 16;
            out.extend_from_slice(&head.to_le_bytes());
            for record in &cell.records {
                record.write(&mut out);
            }
        }
        out.extend_from_slice(&[0; 4]);
        if let Some(heights) = &self.heights {
            out.extend_from_slice(heights);
        }
        out
    }

    /// The payload's size, in bytes.
    pub fn encoded_len(&self) -> usize {
        let cells = self.width as usize * self.height as usize;
        let mut len = HEADER_BYTES
            + cells * CELL_BYTES
            + align4(cells)
            + align4(cells * GRID_BYTES)
            + self.records.len() * TILE_BYTES
            + 4 * (list_words(&self.list_a) + list_words(&self.list_b) + self.objects.iter().map(object_words).sum::<usize>() + self.cells_24.iter().map(cell_words).sum::<usize>())
            + 4 * TILE_BYTES;
        for selector in 0..SELECTORS {
            len += self.rects[selector].len() * RECT_BYTES;
            len += self.quads[selector].len() * QUAD_BYTES;
        }
        len += self.heights.as_ref().map_or(0, Vec::len);
        len
    }
}

/// Reads a `+0x20`/`+0x22` region: records until the zero terminator, which
/// must be the region's last word.
fn read_list(bytes: &[u8], start: usize, end: usize) -> Result<Vec<ListRecord>, Error> {
    let mut records = Vec::new();
    let mut at = start;
    while at + 4 <= end {
        let word = u32_le(bytes, at);
        if word == 0 {
            return (at + 4 == end).then_some(records).ok_or(Error::MapBundleShape("a list region has words after its zero terminator"));
        }
        let records_words = (word >> 16) as usize & 0xff;
        if records_words == 0 || at + records_words * 4 > end {
            return Err(Error::MapBundleShape("a list record's size runs outside its region"));
        }
        records.push(ListRecord { opcode: (word >> 24) as u8, head: [word as u8, (word >> 8) as u8], operands: (1..records_words).map(|index| u32_le(bytes, at + index * 4)).collect() });
        at += records_words * 4;
    }
    Err(Error::MapBundleShape("a list region has no zero terminator"))
}

/// Reads a `+0x24` region: cells until the zero terminator, each holding a
/// record chain that tiles its body exactly.
fn read_cells(bytes: &[u8], start: usize, end: usize) -> Result<Vec<MapCell>, Error> {
    let mut cells = Vec::new();
    let mut at = start;
    while at + 4 <= end {
        let word = u32_le(bytes, at);
        if word == 0 {
            return (at + 4 == end).then_some(cells).ok_or(Error::MapBundleShape("the cell region has words after its zero terminator"));
        }
        let cell_words = u16_le(bytes, at + 2) as usize;
        if cell_words < 2 || at + cell_words * 4 > end {
            return Err(Error::MapBundleShape("a cell's size runs outside the cell region"));
        }
        let body_end = at + cell_words * 4;
        let mut records = Vec::new();
        let mut cursor = at + 4;
        while cursor < body_end {
            let record = u32_le(bytes, cursor);
            let record_words = (record >> 16) as usize & 0xff;
            if record_words == 0 || cursor + record_words * 4 > body_end {
                return Err(Error::MapBundleShape("a cell record's size runs outside its cell"));
            }
            records.push(ListRecord { opcode: (record >> 24) as u8, head: [record as u8, (record >> 8) as u8], operands: (1..record_words).map(|index| u32_le(bytes, cursor + index * 4)).collect() });
            cursor += record_words * 4;
        }
        cells.push(MapCell { y: word as u8, x: (word >> 8) as u8, records });
        at = body_end;
    }
    Err(Error::MapBundleShape("the cell region has no zero terminator"))
}

/// Reads a `+0x28` region: objects until the zero terminator, each holding the
/// body words its head counts.
fn read_objects(bytes: &[u8], start: usize, end: usize) -> Result<Vec<MapObject>, Error> {
    let mut objects = Vec::new();
    let mut at = start;
    while at + 4 <= end {
        let word = u32_le(bytes, at);
        if word == 0 {
            return (at + 4 == end).then_some(objects).ok_or(Error::MapBundleShape("the object region has words after its zero terminator"));
        }
        let body_words = u16_le(bytes, at + 2) as usize;
        if at + 4 + body_words * 4 > end {
            return Err(Error::MapBundleShape("an object's size runs outside the object region"));
        }
        objects.push(MapObject { tag: word as u16, words: (0..body_words).map(|index| u32_le(bytes, at + 4 + index * 4)).collect() });
        at += 4 + body_words * 4;
    }
    Err(Error::MapBundleShape("the object region has no zero terminator"))
}

/// Writes a record region and its zero terminator.
fn write_list(out: &mut Vec<u8>, records: &[ListRecord]) {
    for record in records {
        record.write(out);
    }
    out.extend_from_slice(&[0; 4]);
}

/// The words a record region's records take.
fn list_words(records: &[ListRecord]) -> usize {
    records.iter().map(ListRecord::words).sum()
}

/// The words a `+0x28` object takes, head included, terminator excluded.
fn object_words(object: &MapObject) -> usize {
    1 + object.words.len()
}

/// The words a `+0x24` cell takes, head included, terminator excluded. The
/// cell's own size field counts its head too (`next = cell + u16[+0x2] * 4`).
fn cell_words(cell: &MapCell) -> usize {
    1 + list_words(&cell.records)
}

impl Layout {
    /// Reads the header and checks the section identities the readers assume.
    fn read(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() < HEADER_BYTES {
            return Err(Error::MapBundleTooShort(bytes.len()));
        }
        let width = u32::from(bytes[0]);
        let height = u32::from(bytes[1]);
        let cells = width * height;
        let codes = HEADER_BYTES + cells as usize * CELL_BYTES;
        if u32::from(u16_le(bytes, 0x14)) != 12 + cells {
            return Err(Error::MapBundleShape("the byte-code offset is not 0x30 + width * height * 4"));
        }
        let grid = u32::from(u16_le(bytes, 0x02)) as usize * TILE_BYTES;
        if grid != codes + align4(cells as usize) {
            return Err(Error::MapBundleShape("the grid offset is not the byte-code table's end"));
        }
        let grid_end = align4(grid + cells as usize * GRID_BYTES);
        if grid_end > bytes.len() {
            return Err(Error::MapBundleTooShort(grid_end));
        }
        let rects = [0x04, 0x08, 0x0c, 0x10].map(|at| u32::from(u16_le(bytes, at)) as usize * 4);
        let quads = [0x06, 0x0a, 0x0e, 0x12].map(|at| u32::from(u16_le(bytes, at)) as usize * 4);
        let [list_a, list_b, cells_24, objects, end] = [0x20, 0x22, 0x24, 0x28, 0x2a].map(|at| u32::from(u16_le(bytes, at)) as usize * 4);
        let mut starts = Vec::with_capacity(2 * SELECTORS + 5);
        starts.push(grid_end);
        for selector in 0..SELECTORS {
            starts.push(rects[selector]);
            starts.push(quads[selector]);
        }
        starts.extend([list_a, list_b, objects, cells_24, end]);
        if !starts.windows(2).all(|pair| pair[0] <= pair[1]) {
            return Err(Error::MapBundleShape("the section offsets are not in region order"));
        }
        for selector in 0..SELECTORS {
            if (quads[selector] - rects[selector]) % RECT_BYTES != 0 {
                return Err(Error::MapBundleShape("a rectangle table is not a whole number of 4-byte records"));
            }
            let stop = rects.get(selector + 1).copied().unwrap_or(list_a);
            if (stop - quads[selector]) % QUAD_BYTES != 0 {
                return Err(Error::MapBundleShape("a quad table is not a whole number of 8-byte records"));
            }
        }
        if end > bytes.len() {
            return Err(Error::MapBundleTooShort(end));
        }
        if end < bytes.len() && bytes.len() - end != cells as usize {
            return Err(Error::MapBundleShape("the bytes after the last section are not one height byte per cell"));
        }
        Ok(Self { codes, grid, grid_end, records_end: rects[0], rects, quads, list_a, list_b, objects, cells_24, end })
    }
}

/// Rounds up to a multiple of four.
fn align4(value: usize) -> usize {
    value.next_multiple_of(4)
}
