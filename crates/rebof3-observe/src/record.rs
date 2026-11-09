//! The join of the GPU log to the entries: which entry a textured draw sampled, at what depth and
//! with which CLUT row.
//!
//! The recorder keeps its own copy of VRAM and, for every word, the upload that last wrote it. The
//! log's uploads, fills and copies change both; draws change neither, as in the machine. A draw
//! samples the words its texture page and UVs cover. When every one of them was written by uploads
//! of one entry, the draw proves that entry's depth, and its CLUT attribute names the palette row.

use std::collections::{BTreeMap, HashMap};

use psx_formats::gfx::texture::{Clut, Depth};
use psx_machine::gpu::{Command, Depth as GpuDepth, Draw, Logged, Shape, TextureWindow, VRAM_HEIGHT, VRAM_WIDTH};

use crate::sources::{EntryRef, Sources};

/// "No upload wrote this word" in the owner map.
const NOBODY: u32 = 0;
/// The texture window works in steps of 8 texels (psx-spx "GPU Rendering Attributes", `GP0(E2h)`).
const WINDOW_STEP: u8 = 8;
/// Bits a 5-bit color channel drops from an 8-bit one.
const CHANNEL_DROP: u32 = 3;

/// How a CLUT row's entry was found.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaletteKind {
    /// A type-3 entry uploaded the row's words to VRAM.
    Upload,
    /// A type-0 entry loaded the words into the palette arena, which `GFX_UploadPalette` sends.
    Arena,
}

/// One way the game drew an entry: the depth, the CLUT row and the first draw that showed it.
#[derive(Clone, Debug, PartialEq)]
pub struct Use {
    /// The entry the draws sampled.
    pub entry: EntryRef,
    /// The page depth; `None` is 15-bit.
    pub depth: Option<Depth>,
    /// The CLUT row, for a 4- or 8-bit page.
    pub clut: Option<Clut>,
    /// The CLUT row's words when the palette was named, or at the first draw when it never was.
    pub words: Vec<u16>,
    /// The entry that wrote the CLUT row, and how.
    pub palette: Option<(EntryRef, PaletteKind)>,
    /// The frame of the first draw.
    pub frame: u64,
    /// The `GP0` command byte of the first draw.
    pub opcode: u8,
    /// Draws seen, counting repeats of the same draw in one VRAM state once.
    pub draws: u64,
    /// The VRAM words the draws sampled: left, top, right and bottom, exclusive.
    pub area: [u16; 4],
}

/// Upload counts of a run.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Uploads {
    /// Every `GP0(A0h)` upload.
    pub total: u64,
    /// Uploads whose words an entry of a file read holds at that place.
    pub attributed: u64,
}

/// The state of one run's join.
pub struct Recorder {
    /// The files the game read and their entries.
    pub sources: Sources,
    vram: Vec<u16>,
    /// For each VRAM word, 1 + the index into `writers` of the upload that wrote it.
    owner: Vec<u32>,
    /// The entry each upload came from.
    writers: Vec<Option<EntryRef>>,
    /// Draws already joined in the current VRAM state, and the use each one counted toward.
    seen: HashMap<DrawKey, Option<usize>>,
    uses: Vec<Use>,
    by_use: HashMap<(EntryRef, Option<u32>, Option<Clut>), usize>,
    /// Upload counts.
    pub uploads: Uploads,
    /// Textured draws seen.
    pub textured_draws: u64,
}

/// What makes two draws the same sample: page, depth, CLUT and the words covered.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct DrawKey {
    page: (u16, u16),
    bits: Option<u32>,
    clut: Clut,
    area: [u16; 4],
}

impl Recorder {
    /// A recorder with an empty VRAM.
    pub fn new(sources: Sources) -> Self {
        Self {
            sources,
            vram: vec![0; VRAM_WIDTH * VRAM_HEIGHT],
            owner: vec![NOBODY; VRAM_WIDTH * VRAM_HEIGHT],
            writers: Vec::new(),
            seen: HashMap::new(),
            uses: Vec::new(),
            by_use: HashMap::new(),
            uploads: Uploads::default(),
            textured_draws: 0,
        }
    }

    /// The uses found so far, ordered by entry and then by draw count, most first.
    pub fn uses(&self) -> Vec<Use> {
        let mut uses = self.uses.clone();
        uses.sort_by(|a, b| a.entry.cmp(&b.entry).then(b.draws.cmp(&a.draws)).then(a.frame.cmp(&b.frame)));
        uses
    }

    /// Joins one frame's GPU log.
    pub fn log(&mut self, log: &[Logged]) {
        for logged in log {
            match &logged.command {
                Command::Upload { rect, pixels } => {
                    let writer = self.sources.upload(rect.x, rect.y, rect.width, rect.height, pixels);
                    self.uploads.total += 1;
                    self.uploads.attributed += u64::from(writer.is_some());
                    self.writers.push(writer);
                    let owner = self.writers.len() as u32;
                    let words = usize::from(rect.width);
                    for row in 0..usize::from(rect.height) {
                        for column in 0..words {
                            let at = index(usize::from(rect.x) + column, usize::from(rect.y) + row);
                            self.vram[at] = pixels.get(row * words + column).copied().unwrap_or(0);
                            self.owner[at] = owner;
                        }
                    }
                    self.seen.clear();
                }
                Command::Fill { rect, color } => {
                    let word = rgb15(*color);
                    for row in 0..usize::from(rect.height) {
                        for column in 0..usize::from(rect.width) {
                            let at = index(usize::from(rect.x) + column, usize::from(rect.y) + row);
                            self.vram[at] = word;
                            self.owner[at] = NOBODY;
                        }
                    }
                    self.seen.clear();
                }
                Command::Copy { from_x, from_y, to } => {
                    let mut words = Vec::with_capacity(usize::from(to.width) * usize::from(to.height));
                    for row in 0..usize::from(to.height) {
                        for column in 0..usize::from(to.width) {
                            let at = index(usize::from(*from_x) + column, usize::from(*from_y) + row);
                            words.push((self.vram[at], self.owner[at]));
                        }
                    }
                    let mut source = words.into_iter();
                    for row in 0..usize::from(to.height) {
                        for column in 0..usize::from(to.width) {
                            let at = index(usize::from(to.x) + column, usize::from(to.y) + row);
                            (self.vram[at], self.owner[at]) = source.next().unwrap_or((0, NOBODY));
                        }
                    }
                    self.seen.clear();
                }
                Command::Draw(draw) => self.draw(logged.frame, draw),
                _ => {}
            }
        }
    }

    fn draw(&mut self, frame: u64, draw: &Draw) {
        let Some(texture) = draw.texture else { return };
        self.textured_draws += 1;
        let depth = match texture.page.depth {
            GpuDepth::Bits4 => Some(Depth::Four),
            GpuDepth::Bits8 => Some(Depth::Eight),
            GpuDepth::Bits15 => None,
        };
        let Some(([u_from, u_to], [v_from, v_to])) = uv_bounds(draw, texture.window) else { return };
        let per_word = depth.map_or(1, Depth::texels_per_word) as u16;
        let left = texture.page.x + u_from / per_word;
        let right = texture.page.x + (u_to - 1) / per_word + 1;
        let area = [left, texture.page.y + v_from, right, texture.page.y + v_to];
        let clut = Clut { x: texture.clut.x, y: texture.clut.y };
        let key = DrawKey { page: (texture.page.x, texture.page.y), bits: depth.map(Depth::bits), clut, area };
        if let Some(counted) = self.seen.get(&key) {
            if let Some(at) = *counted {
                self.uses[at].draws += 1;
            }
            return;
        }
        let counted = self.sole_entry(area).map(|entry| self.count(entry, depth, clut, area, frame, draw.opcode));
        self.seen.insert(key, counted);
    }

    /// The one entry whose uploads wrote every word of `area`.
    fn sole_entry(&self, [left, top, right, bottom]: [u16; 4]) -> Option<EntryRef> {
        let mut found = None;
        let mut last_owner = NOBODY;
        for y in top..bottom {
            for x in left..right {
                let owner = self.owner[index(usize::from(x), usize::from(y))];
                if owner == last_owner {
                    continue;
                }
                last_owner = owner;
                let entry = self.writers.get((owner as usize).checked_sub(1)?).copied().flatten()?;
                if found.is_some_and(|found| found != entry) {
                    return None;
                }
                found = Some(entry);
            }
        }
        found
    }

    /// Counts one draw of `entry`, and returns the index of its use.
    fn count(
        &mut self,
        entry: EntryRef,
        depth: Option<Depth>,
        clut: Clut,
        area: [u16; 4],
        frame: u64,
        opcode: u8,
    ) -> usize {
        let clut = depth.map(|_| clut);
        let words = match (depth, clut) {
            (Some(depth), Some(clut)) => self.row(clut, depth),
            _ => Vec::new(),
        };
        let key = (entry, depth.map(Depth::bits), clut);
        let at = match self.by_use.get(&key) {
            Some(&at) => at,
            None => {
                self.uses.push(Use {
                    entry,
                    depth,
                    clut,
                    words: words.clone(),
                    palette: None,
                    frame,
                    opcode,
                    draws: 0,
                    area,
                });
                self.by_use.insert(key, self.uses.len() - 1);
                self.uses.len() - 1
            }
        };
        if let (Some(depth), Some(clut)) = (depth, clut)
            && self.uses[at].palette.is_none()
            && let Some(palette) = self.palette(clut, depth, &words)
        {
            self.uses[at].palette = Some(palette);
            self.uses[at].words = words;
        }
        let used = &mut self.uses[at];
        used.draws += 1;
        used.area = [
            used.area[0].min(area[0]),
            used.area[1].min(area[1]),
            used.area[2].max(area[2]),
            used.area[3].max(area[3]),
        ];
        at
    }

    /// The words of the CLUT row at `clut`.
    fn row(&self, clut: Clut, depth: Depth) -> Vec<u16> {
        (0..depth.colors()).map(|at| self.vram[index(usize::from(clut.x) + at, usize::from(clut.y))]).collect()
    }

    /// The entry that wrote the CLUT row's `words`: a type-3 entry when its upload wrote all of
    /// them, else an arena bank of a file read that holds those words at that place.
    fn palette(&self, clut: Clut, depth: Depth, words: &[u16]) -> Option<(EntryRef, PaletteKind)> {
        let right = clut.x + depth.colors() as u16;
        if let Some(entry) = self.sole_entry([clut.x, clut.y, right, clut.y + 1]) {
            return Some((entry, PaletteKind::Upload));
        }
        self.sources.arena_bank(clut, depth, words).map(|entry| (entry, PaletteKind::Arena))
    }

    /// The uses of each entry, keyed by the file's disc path and the slot.
    pub fn by_entry(&self) -> BTreeMap<(String, usize), Vec<Use>> {
        let mut entries: BTreeMap<(String, usize), Vec<Use>> = BTreeMap::new();
        for used in self.uses() {
            let path = self.sources.files[used.entry.file].path.clone();
            entries.entry((path, used.entry.slot)).or_default().push(used);
        }
        entries
    }
}

/// The texel range a draw samples, `[from, to)` in U and in V, after the texture window.
fn uv_bounds(draw: &Draw, window: TextureWindow) -> Option<([u16; 2], [u16; 2])> {
    let first = draw.vertices.first()?;
    let (u, v) = match draw.shape {
        Shape::Rect { width, height } => {
            let span = |start: u8, length: u16, flipped: bool| -> [u16; 2] {
                let start = u16::from(start);
                if flipped {
                    [start.saturating_sub(length.saturating_sub(1)), start + 1]
                } else {
                    [start, (start + length).min(256)]
                }
            };
            let texture = draw.texture?;
            (span(first.u, width, texture.flip_x), span(first.v, height, texture.flip_y))
        }
        _ => {
            let span = |values: Vec<u8>| -> [u16; 2] {
                let low = values.iter().copied().min().map_or(0, u16::from);
                let high = values.iter().copied().max().map_or(0, u16::from);
                [low, high.max(low + 1)]
            };
            let us = draw.vertices.iter().map(|vertex| windowed(vertex.u, window.mask_x, window.offset_x)).collect();
            let vs = draw.vertices.iter().map(|vertex| windowed(vertex.v, window.mask_y, window.offset_y)).collect();
            (span(us), span(vs))
        }
    };
    (u[0] < u[1] && v[0] < v[1]).then_some((u, v))
}

/// A texture coordinate through the texture window: `(uv & !(mask * 8)) | ((offset & mask) * 8)`.
fn windowed(value: u8, mask: u8, offset: u8) -> u8 {
    (value & !mask.wrapping_mul(WINDOW_STEP)) | (offset & mask).wrapping_mul(WINDOW_STEP)
}

/// The VRAM index of (`x`, `y`), wrapping at the edges as the GPU does.
fn index(x: usize, y: usize) -> usize {
    (y % VRAM_HEIGHT) * VRAM_WIDTH + (x % VRAM_WIDTH)
}

/// A 24-bit `0xBBGGRR` fill color as a 15-bit VRAM word.
fn rgb15(color: u32) -> u16 {
    let channel = |shift: u32| (color >> (shift + CHANNEL_DROP)) & 0x1F;
    (channel(0) | channel(8) << 5 | channel(16) << 10) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_texture_window_replaces_the_masked_bits() {
        assert_eq!(windowed(0x37, 0, 0), 0x37);
        // Mask 3 clears bits 3 and 4; offset 1 sets bit 3.
        assert_eq!(windowed(0x1F, 0x03, 0x01), 0x0F);
    }

    #[test]
    fn a_fill_color_drops_the_low_bits_of_each_channel() {
        assert_eq!(rgb15(0x00FF_FFFF), 0x7FFF);
        assert_eq!(rgb15(0x0000_00F8), 0x001F);
    }
}
