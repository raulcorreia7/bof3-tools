//! Texture joins resolved in a replayed VRAM.
//!
//! A rule names a texture page and a CLUT row; the replay says which entries wrote them. The
//! texture is the upload that wrote the page's first word, and the palette is the one entry that
//! wrote the whole row, `depth.colors()` words. A row with words from two entries, or none, does
//! not join. Because the replay keeps load order, a block another archive loaded serves a row only
//! when it is the one the scene left there.
//!
//! The rules are the ones in [`crate::texture`]: the literal draw sites ([`super::SITES`]) of the
//! archives the scene loads, and the world-map tile descriptors (`func_80155560`) the caller reads
//! from the scene's area map bundles.

use psx_formats::gfx::texture::{Clut, Color, Depth, Tpage};

use super::{SITES, Site, terrain_descriptor};
use crate::replay::scenes::Scene;
use crate::replay::{Kind, Vram, Writer};

/// What named a join.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Rule {
    /// A draw site with a literal page and CLUT.
    Site(&'static Site),
    /// A world-map tile descriptor word.
    Terrain(u32),
}

impl Rule {
    /// One line of evidence for the join.
    pub fn evidence(&self) -> String {
        match self {
            Rule::Site(site) => format!("{} overlay {} function {:#010x} ({}): tpage {:#06x}, clut {:#06x}", site.archive, site.overlay, site.function, site.source, site.tpage, site.clut),
            Rule::Terrain(descriptor) => {
                format!("world-map tile descriptor {descriptor:#010x} (func_80155560)")
            }
        }
    }
}

/// A texture page and CLUT row that a rule names, resolved to the entries that wrote them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Join {
    /// The upload that wrote the page.
    pub texture: Writer,
    /// The entry that wrote the whole CLUT row.
    pub palette: Writer,
    /// The page's origin in VRAM words and rows.
    pub page: (u16, u16),
    /// The CLUT row's VRAM position.
    pub clut: Clut,
    /// The texel depth.
    pub depth: Depth,
    /// The rule that names the page and the row.
    pub rule: Rule,
}

/// The upload and palette that the page at `page_x`, `page_y` and the CLUT row `clut` resolve to.
pub fn resolve(vram: &Vram, page_x: u16, page_y: u16, clut: Clut, depth: Depth) -> Option<(Writer, Writer)> {
    let (x, y) = (usize::from(page_x), usize::from(page_y));
    let texture = vram.writer(x, y).filter(|_| vram.kind(x, y) == Some(Kind::Upload))?;
    let palette = vram.clut_writer(clut, depth)?;
    Some((texture.clone(), palette.clone()))
}

/// Every join the rules prove in a replayed scene: the sites of the archives it loads and the
/// tile `descriptors` of its area map bundles. Repeated joins of one texture, row and depth are
/// kept once.
pub fn scene_joins(scene: &Scene, vram: &Vram, descriptors: &[u32]) -> Vec<Join> {
    let mut joins: Vec<Join> = Vec::new();
    let mut push = |join: Join| {
        let seen = joins.iter().any(|old| old.texture == join.texture && old.clut == join.clut && old.depth == join.depth);
        if !seen {
            joins.push(join);
        }
    };
    for path in scene.archives() {
        for site in SITES.iter().filter(|site| path.ends_with(&format!("/{}", site.archive))) {
            let Some(tpage) = Tpage::from_word(site.tpage) else { continue };
            let Color::Indexed(depth) = tpage.color else { continue };
            let clut = Clut::from_word(site.clut);
            let Some((texture, palette)) = resolve(vram, tpage.x, tpage.y, clut, depth) else { continue };
            if texture.as_entry().is_some_and(|(archive, _)| site.reads_page_of(archive)) {
                push(Join { texture, palette, page: (tpage.x, tpage.y), clut, depth, rule: Rule::Site(site) });
            }
        }
    }
    for &descriptor in descriptors {
        let Some((page_x, page_y, clut, depth)) = terrain_descriptor(descriptor) else { continue };
        if let Some((texture, palette)) = resolve(vram, page_x, page_y, clut, depth) {
            push(Join { texture, palette, page: (page_x, page_y), clut, depth, rule: Rule::Terrain(descriptor) });
        }
    }
    joins
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Entry;

    fn entry(file_type: u16, ram_ptr: u32, size: usize) -> Entry {
        Entry { size: size as u32, offset: 0, ram_ptr, first4: 0, file_type }
    }

    #[test]
    fn a_page_and_a_row_resolve_to_their_writers() {
        let mut vram = Vram::new();
        let sheet: Vec<u8> = vec![0x11; 2048];
        // Rectangle column 22, row 8: VRAM (704, 256).
        let block: Vec<u8> = (0..16u16).flat_map(|word| (0x8000 | word).to_le_bytes()).collect();
        let entries = [entry(3, (22 << 24) | (8 << 16) | (1 << 8), sheet.len()), entry(0, 0x8003_3F40, block.len())];
        vram.load("BIN/WORLD00/AREA024.EMI", &entries, &[&sheet, &block]);
        vram.reset_palette();
        vram.upload_palette();

        let clut = Clut::new(160, 483).expect("a CLUT");
        let (texture, palette) = resolve(&vram, 704, 256, clut, Depth::Four).expect("a join");
        assert_eq!(texture, Writer::entry("BIN/WORLD00/AREA024.EMI", 0));
        assert_eq!(palette, Writer::entry("BIN/WORLD00/AREA024.EMI", 1));
        assert!(resolve(&vram, 704, 256, clut, Depth::Eight).is_none(), "the 8-bit row is only partly written");
        assert!(resolve(&vram, 0, 0, clut, Depth::Four).is_none(), "nothing wrote the page");
        assert!(resolve(&vram, 160, 483, clut, Depth::Four).is_none(), "a palette is not a texture page");
    }
}
