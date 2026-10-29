//! Which palette row a texture is drawn with: the palette arena, the texture sheets, and the
//! draw rules that name both.
//!
//! The palette arena is a 16 KB band of RAM. The engine copies it to a working copy and
//! uploads that copy with one `LoadImage` of `rect(0, 480, 256, 32)` (exe `func_8014E22C`,
//! `src/slus/func_8014E22C.c`). A type-0 palette loaded at RAM `ram` therefore
//! sits at arena word `(ram - ARENA_BASE) / 2`, which is VRAM column `% 256` and row
//! `480 + / 256`. A type-3 entry is a texture sheet placed by its load argument
//! ([`sheet`]).
//!
//! A texture's palette is named by a draw rule, never by the texture payload:
//!
//! - [`SITES`] lists the functions whose texture page and CLUT are both literals. A site joins
//!   when its page is covered by exactly one sheet and its CLUT row is held by exactly one bank.
//! - [`terrain_joins`] applies the world-map tile-descriptor rule: bit 31 selects the depth,
//!   bits 24-27 the CLUT index and bits 28-29 the texture-page bank. It joins the same way.
//!
//! Banks are looked up over the resident set of palette banks ([`ResidentBank`]) the caller
//! loaded, so a bank one archive carries can serve another archive's texture (the battle icon
//! rows). The texture's own archive wins when it holds the row; otherwise identical banks
//! collapse and disagreeing banks make the row ambiguous. The rule never guesses: more than one
//! sheet or more than one distinct bank leaves the texture unjoined.
//!
//! The encoded words assume graph type 0, the runtime the US disc selects
//! (`GetGraphType()` reads `D_8018B15C`; `SetVideoMode(MODE_NTSC)` and `_reset(3)` leave it
//! zero). A site that writes the graph-type-1/2 encoding is decoded with the same 4bpp/8bpp
//! page fields the research join used, and the depth comes from the word's color mode.

use psx_formats::gfx::texture::{Clut, Color, Depth, Tpage};
use psx_formats::gfx::vram::{self, Descriptor};

/// RAM address of the palette arena (`wiki: formats/area#palette-arena`).
pub const ARENA_BASE: u32 = 0x8003_3800;
/// Bytes in the palette arena.
pub const ARENA_BYTES: u32 = 0x4000;
/// VRAM row of the arena's first row (`rect(0, 0x1E0, 0x100, 0x20)`).
pub const ARENA_VRAM_ROW: u16 = 480;
/// Words per arena row, which is the upload rectangle's width.
pub const ARENA_ROW_WORDS: u16 = 256;

/// The VRAM row of the first world-map terrain CLUT row.
pub const TERRAIN_CLUT_ROW: u16 = 483;
/// The VRAM column of the first world-map terrain texture page.
pub const TERRAIN_PAGE_X: u16 = 320;
/// VRAM words between world-map terrain page banks (descriptor bits 28-29).
pub const TERRAIN_PAGE_STEP: u16 = 128;
/// The VRAM row of every world-map terrain texture page.
pub const TERRAIN_PAGE_Y: u16 = 256;

/// Descriptor bits 24-27 hold the terrain CLUT index.
const TERRAIN_CLUT_INDEX_SHIFT: u32 = 24;
/// Descriptor bits 28-29 hold the terrain page bank.
const TERRAIN_PAGE_BANK_SHIFT: u32 = 28;
/// Descriptor bit 31 selects the terrain depth: clear is 8bpp, set is 4bpp.
const TERRAIN_DEPTH_BIT: u32 = 31;

/// A type-0 palette in the arena: its entry, and where its first word sits in the arena.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Bank {
    /// EMI slot of the palette entry.
    pub slot: usize,
    /// Arena word of the bank's first color.
    pub arena_word: usize,
    /// Color words in the bank.
    pub words: usize,
}

impl Bank {
    /// The bank loaded from a type-0 entry at `ram` with `payload_bytes` bytes, or `None`
    /// when it does not load into the arena.
    pub fn new(slot: usize, ram: u32, payload_bytes: usize) -> Option<Self> {
        if !(ARENA_BASE..ARENA_BASE + ARENA_BYTES).contains(&ram) {
            return None;
        }
        Some(Self { slot, arena_word: ((ram - ARENA_BASE) / 2) as usize, words: payload_bytes / 2 })
    }

    /// Whether the whole CLUT row at `clut`, `depth.colors()` words long, lies in this bank.
    pub fn holds(&self, clut: Clut, depth: Depth) -> bool {
        let Some(first) = arena_word(clut) else {
            return false;
        };
        first >= self.arena_word && first + depth.colors() <= self.arena_word + self.words
    }
}

/// A palette bank with the archive that loads it and the color words it holds.
///
/// A caller builds the resident set by loading the archives it has; nothing here assumes which
/// archives those are. Banks that load the same words at the same arena position are one bank.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResidentBank {
    /// Disc path of the archive that loads the bank (`BIN/BATTLE/BATL_OVR.EMI`).
    pub archive: String,
    /// The bank's entry and arena span.
    pub bank: Bank,
    /// The color words, in stored order.
    pub values: Vec<u16>,
}

impl ResidentBank {
    /// The bank a type-0 payload loaded at `ram` holds, or `None` when it does not load into
    /// the arena.
    pub fn new(archive: impl Into<String>, slot: usize, ram: u32, payload: &[u8]) -> Option<Self> {
        let bank = Bank::new(slot, ram, payload.len())?;
        let (pairs, _) = payload.as_chunks::<2>();
        let values = pairs.iter().map(|pair| u16::from_le_bytes(*pair)).collect();
        Some(Self { archive: archive.into(), bank, values })
    }

    /// Whether the whole CLUT row at `clut`, `depth.colors()` words long, lies in this bank.
    pub fn holds(&self, clut: Clut, depth: Depth) -> bool {
        self.bank.holds(clut, depth)
    }

    /// The row's `depth.colors()` color words, or `None` when the row is not in this bank.
    pub fn row(&self, clut: Clut, depth: Depth) -> Option<&[u16]> {
        if !self.holds(clut, depth) {
            return None;
        }
        let first = arena_word(clut)?;
        let start = first - self.bank.arena_word;
        self.values.get(start..start + depth.colors())
    }
}

/// The arena word of a CLUT row, or `None` when the row is above the arena.
pub fn arena_word(clut: Clut) -> Option<usize> {
    let row = clut.y.checked_sub(ARENA_VRAM_ROW)?;
    Some(usize::from(row) * usize::from(ARENA_ROW_WORDS) + usize::from(clut.x))
}

/// The VRAM position of arena word `arena_word`: the first color of a bank loaded there.
pub fn clut_at(arena_word: usize) -> Clut {
    let row = arena_word / usize::from(ARENA_ROW_WORDS);
    Clut { x: (arena_word % usize::from(ARENA_ROW_WORDS)) as u16, y: ARENA_VRAM_ROW + row as u16 }
}

/// A type-3 entry's place in VRAM, in words and rows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Sheet {
    /// EMI slot of the upload.
    pub slot: usize,
    /// Left edge in VRAM words.
    pub x: u16,
    /// Top edge in VRAM rows.
    pub y: u16,
    /// Width in VRAM words.
    pub width: u16,
    /// Height in VRAM rows.
    pub height: u16,
}

impl Sheet {
    /// Whether the sheet covers the VRAM word at `(x, y)`.
    pub fn covers(&self, x: u16, y: u16) -> bool {
        (self.x..self.x + self.width).contains(&x) && (self.y..self.y + self.height).contains(&y)
    }
}

/// The sheet a type-3 entry uploads, from its load argument and payload length, or `None`
/// when the payload is not a whole number of rectangle rows.
pub fn sheet(slot: usize, load_arg: u32, payload_bytes: usize) -> Option<Sheet> {
    let descriptor = Descriptor::from_load_argument(load_arg);
    let per_row = usize::from(descriptor.chunks_per_row);
    let rectangles = payload_bytes / vram::RECTANGLE_BYTES;
    if per_row == 0 || rectangles == 0 || !rectangles.is_multiple_of(per_row) {
        return None;
    }
    let side = vram::RECTANGLE_SIDE as u16;
    Some(Sheet {
        slot,
        x: u16::from(descriptor.column) * side,
        y: u16::from(descriptor.row) * side,
        width: per_row as u16 * side,
        height: (rectangles / per_row) as u16 * side,
    })
}

/// A draw site whose texture page and CLUT are both literal in one function.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Site {
    /// The archive the overlay is in, as the disc path ends (`WORLD00/AREA024.EMI`).
    pub archive: &'static str,
    /// The overlay entry the function is in.
    pub overlay: usize,
    /// The function's address.
    pub function: u32,
    /// The source lines that write the page and the CLUT.
    pub source: &'static str,
    /// The `tpage` word the function writes.
    pub tpage: u16,
    /// The `clut` word the function writes.
    pub clut: u16,
}

/// Builds a [`Site`] table entry.
const fn literal(
    archive: &'static str,
    overlay: usize,
    function: u32,
    source: &'static str,
    tpage: u16,
    clut: u16,
) -> Site {
    Site { archive, overlay, function, source, tpage, clut }
}

/// Draw sites with a literal page and CLUT in the same function, and no runtime condition.
/// Sites outside this table are in `wiki: formats/assets#texture-evidence`.
///
/// The nine area-sprite sites write both page encodings; both words are listed because the
/// graph-type-1/2 word decodes to a 4bpp page over the second sheet, as the research join
/// recorded. AREA030's effect and icon sites name the graph-type-0 words.
pub const SITES: &[Site] = &[
    literal("WORLD00/AREA024.EMI", 14, 0x801F_2DF8, "src/world00/area024/14/sprite_draw.c:423-424", 0x003B, 0x78CA),
    // The nine area sprite pages: one 8bpp and one 4bpp sheet, the same CLUT row.
    literal("WORLD00/AREA016.EMI", 13, 0x801F_39D8, "src/world00/area016/13/func_801F39D8.c:9-24", 0x009C, 0x7B80),
    literal("WORLD00/AREA016.EMI", 13, 0x801F_39D8, "src/world00/area016/13/func_801F39D8.c:9-24", 0x022C, 0x7B80),
    literal("WORLD01/AREA045.EMI", 13, 0x801F_39D8, "src/world01/area045/13/func_801F39D8.c:7-22", 0x009C, 0x7B80),
    literal("WORLD01/AREA045.EMI", 13, 0x801F_39D8, "src/world01/area045/13/func_801F39D8.c:7-22", 0x022C, 0x7B80),
    literal("WORLD01/AREA065.EMI", 13, 0x801F_3A10, "src/world01/area065/13/func_801F3A10.c:7-22", 0x009C, 0x7B80),
    literal("WORLD01/AREA065.EMI", 13, 0x801F_3A10, "src/world01/area065/13/func_801F3A10.c:7-22", 0x022C, 0x7B80),
    literal("WORLD02/AREA087.EMI", 13, 0x801F_3A0C, "src/world02/area087/13/func_801F3A0C.c:9-23", 0x009C, 0x7B80),
    literal("WORLD02/AREA087.EMI", 13, 0x801F_3A0C, "src/world02/area087/13/func_801F3A0C.c:9-23", 0x022C, 0x7B80),
    literal("WORLD02/AREA088.EMI", 13, 0x801F_453C, "src/world02/area088/13/func_801F453C.c:9-23", 0x009C, 0x7B80),
    literal("WORLD02/AREA088.EMI", 13, 0x801F_453C, "src/world02/area088/13/func_801F453C.c:9-23", 0x022C, 0x7B80),
    literal("WORLD03/AREA115.EMI", 13, 0x801F_39DC, "src/world03/area115/13/func_801F39DC.c:9-23", 0x009C, 0x7B80),
    literal("WORLD03/AREA115.EMI", 13, 0x801F_39DC, "src/world03/area115/13/func_801F39DC.c:9-23", 0x022C, 0x7B80),
    literal("WORLD03/AREA121.EMI", 14, 0x801F_39F0, "src/world03/area121/14/func_801F39F0.c:9-23", 0x009C, 0x7B80),
    literal("WORLD03/AREA121.EMI", 14, 0x801F_39F0, "src/world03/area121/14/func_801F39F0.c:9-23", 0x022C, 0x7B80),
    literal("WORLD03/AREA151.EMI", 13, 0x801F_3948, "src/world03/area151/13/func_801F3948.c:8-22", 0x009C, 0x7B80),
    literal("WORLD03/AREA151.EMI", 13, 0x801F_3948, "src/world03/area151/13/func_801F3948.c:8-22", 0x022C, 0x7B80),
    literal("WORLD04/AREA152.EMI", 13, 0x801F_389C, "src/world04/area152/13/func_801F389C.c:10-24", 0x009C, 0x7B80),
    literal("WORLD04/AREA152.EMI", 13, 0x801F_389C, "src/world04/area152/13/func_801F389C.c:10-24", 0x022C, 0x7B80),
    // AREA016's effect quad and AREA030's effect quads and menu icons.
    literal(
        "WORLD00/AREA016.EMI",
        13,
        0x801F_4768,
        "src/world00/area016/13/func_801F4768.c:129,132-133",
        0x005B,
        0x78CB,
    ),
    literal(
        "WORLD00/AREA030.EMI",
        5,
        0x800F_5E44,
        "src/world00/area030/05/C.c:330-342; src/world00/area030/04/func_801D2C34.c:15-23",
        0x0095,
        0x7A40,
    ),
    literal("WORLD00/AREA030.EMI", 5, 0x800F_5E44, "src/world00/area030/05/C.c:330-342", 0x0095, 0x7A80),
    literal(
        "WORLD00/AREA030.EMI",
        4,
        0x801D_799C,
        "src/world00/area030/04/func_801D799C.c:15-21; src/world00/area030/04/data.c:270",
        0x001D,
        0x7887,
    ),
    literal("WORLD00/AREA030.EMI", 4, 0x801D_8248, "src/world00/area030/04/func_801D8248.c:13-20", 0x002D, 0x7887),
    // MAGIC218's effect page and CLUT, from retail assembly.
    literal(
        "BMAGIC/MAGIC218.EMI",
        3,
        0x801E_F250,
        "asm/emi/bmagic/magic218/03/func_801EF250.s:148-208",
        0x00BD,
        0x7E80,
    ),
];

/// What named a join: a literal draw site or the world-map terrain descriptor rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Rule {
    /// A draw site with a literal page and CLUT.
    Site(&'static Site),
    /// A world-map tile descriptor (`func_80155560`) and the page it selected.
    Terrain { page_x: u16, page_y: u16 },
}

/// A texture drawn with one palette row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Join<'a> {
    /// EMI slot of the texture sheet the page is covered by.
    pub texture: usize,
    /// The palette bank that holds the row.
    pub palette: &'a ResidentBank,
    /// The CLUT row's VRAM position.
    pub clut: Clut,
    /// The texel depth the page's color mode selects.
    pub depth: Depth,
    /// The rule that names both.
    pub rule: Rule,
}

/// The joins of one archive, given its texture sheets and the resident palette banks.
///
/// A site joins only when exactly one sheet covers its page and exactly one distinct bank holds
/// its CLUT row. Anything else is left out, so the result never guesses between candidates.
///
/// The texture's own archive wins when it holds the row; otherwise the resident set is
/// consulted and identical banks collapse. A cross-archive bank is what serves the battle icon
/// rows, whose pages are in `BATTLE.EMI` and whose palettes are in `BATL_OVR.EMI` and
/// `BATL_DRA.EMI`.
pub fn joins<'a>(archive: &str, sheets: &[Sheet], banks: &[&'a ResidentBank]) -> Vec<Join<'a>> {
    SITES
        .iter()
        .filter(|site| archive == site.archive || archive.ends_with(&format!("/{}", site.archive)))
        .filter_map(|site| {
            let tpage = Tpage::from_word(site.tpage)?;
            let Color::Indexed(depth) = tpage.color else {
                return None;
            };
            let clut = Clut::from_word(site.clut);
            let texture = only(sheets.iter().filter(|sheet| sheet.covers(tpage.x, tpage.y)))?;
            let palette = holder(archive, banks, clut, depth)?;
            Some(Join { texture: texture.slot, palette, clut, depth, rule: Rule::Site(site) })
        })
        .collect()
}

/// The joins implied by a world-map bundle's tile descriptor words.
///
/// Each descriptor names one row and one page (`func_80155560`): bit 31 selects the depth,
/// bits 24-27 the CLUT index, and bits 28-29 the page bank. In 8bpp the CLUT is
/// `(0, 483 + index)`; in 4bpp it is `(16 * index, 483)`. The page is
/// `(320 + 128 * bank, 256)`. The same uniqueness rule applies: one covering sheet and one
/// holding bank, or no join. Repeated descriptors yield one join.
pub fn terrain_joins<'a>(
    archive: &str,
    records: &[u32],
    sheets: &[Sheet],
    banks: &[&'a ResidentBank],
) -> Vec<Join<'a>> {
    let mut joins: Vec<Join<'a>> = Vec::new();
    for &descriptor in records {
        let Some((page_x, page_y, clut, depth)) = terrain_descriptor(descriptor) else {
            continue;
        };
        let Some(texture) = only(sheets.iter().filter(|sheet| sheet.covers(page_x, page_y))) else {
            continue;
        };
        let Some(palette) = holder(archive, banks, clut, depth) else {
            continue;
        };
        let join = Join { texture: texture.slot, palette, clut, depth, rule: Rule::Terrain { page_x, page_y } };
        if !joins.iter().any(|seen| seen.texture == join.texture && seen.clut == join.clut) {
            joins.push(join);
        }
    }
    joins
}

/// The page and CLUT one world-map tile descriptor names, or `None` for an empty descriptor.
pub fn terrain_descriptor(descriptor: u32) -> Option<(u16, u16, Clut, Depth)> {
    if descriptor == 0 {
        return None;
    }
    let index = ((descriptor >> TERRAIN_CLUT_INDEX_SHIFT) & 0xF) as u16;
    let (clut, depth) = if descriptor >> TERRAIN_DEPTH_BIT == 0 {
        (Clut::new(0, TERRAIN_CLUT_ROW + index)?, Depth::Eight)
    } else {
        (Clut::new(index * 16, TERRAIN_CLUT_ROW)?, Depth::Four)
    };
    let bank = ((descriptor >> TERRAIN_PAGE_BANK_SHIFT) & 3) as u16;
    Some((TERRAIN_PAGE_X + bank * TERRAIN_PAGE_STEP, TERRAIN_PAGE_Y, clut, depth))
}

/// The one bank that holds the row: the archive's own when it has one, else the family's.
fn holder<'a>(archive: &str, banks: &[&'a ResidentBank], clut: Clut, depth: Depth) -> Option<&'a ResidentBank> {
    let own: Vec<&ResidentBank> =
        banks.iter().copied().filter(|bank| bank.archive == archive && bank.holds(clut, depth)).collect();
    if !own.is_empty() {
        return one(own.into_iter());
    }
    one(banks.iter().copied().filter(|bank| bank.holds(clut, depth)))
}

/// The single distinct bank among `banks`, comparing the arena span and the color words.
fn one<'a>(banks: impl Iterator<Item = &'a ResidentBank>) -> Option<&'a ResidentBank> {
    let mut distinct: Vec<&ResidentBank> = Vec::new();
    for bank in banks {
        if !distinct.iter().any(|seen| same_bank(seen, bank)) {
            distinct.push(bank);
        }
    }
    match distinct.as_slice() {
        [bank] => Some(bank),
        _ => None,
    }
}

/// Whether two resident banks load the same words at the same arena position.
fn same_bank(left: &ResidentBank, right: &ResidentBank) -> bool {
    left.bank.arena_word == right.bank.arena_word && left.values == right.values
}

/// The single item of `items`, or `None` when there are none or several.
fn only<T>(items: impl Iterator<Item = T>) -> Option<T> {
    let mut items = items;
    let first = items.next()?;
    items.next().is_none().then_some(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bank_with(archive: &str, slot: usize, ram: u32, words: u16, seed: u16) -> ResidentBank {
        let payload: Vec<u8> = (0..words).flat_map(|word| (0x8000 | (word + seed)).to_le_bytes()).collect();
        ResidentBank::new(archive, slot, ram, &payload).expect("the bank loads into the arena")
    }

    fn bank(archive: &str, slot: usize, ram: u32, words: u16) -> ResidentBank {
        bank_with(archive, slot, ram, words, 0)
    }

    fn sheet(slot: usize, x: u16, y: u16) -> Sheet {
        Sheet { slot, x, y, width: 64, height: 64 }
    }

    #[test]
    fn a_terrain_descriptor_splits_into_depth_page_and_clut() {
        // 8bpp, CLUT index 5, page bank 2: page (576, 256), row 488.
        let descriptor = (2 << 28) | (5 << 24);
        let (page_x, page_y, clut, depth) = terrain_descriptor(descriptor).expect("a live descriptor");
        assert_eq!((page_x, page_y), (576, 256));
        assert_eq!((clut.x, clut.y), (0, 488));
        assert_eq!(depth, Depth::Eight);

        // 4bpp, CLUT index 3, page bank 1: page (448, 256), column 48.
        let descriptor = (1 << 31) | (1 << 28) | (3 << 24);
        let (page_x, _, clut, depth) = terrain_descriptor(descriptor).expect("a live descriptor");
        assert_eq!(page_x, 448);
        assert_eq!((clut.x, clut.y), (48, 483));
        assert_eq!(depth, Depth::Four);
        assert_eq!(terrain_descriptor(0), None);
    }

    #[test]
    fn the_terrain_rule_refuses_overlapping_sheets_and_banks() {
        let archive = "BIN/WORLD00/AREA000.EMI";
        let sheets = [sheet(7, 576, 256), sheet(8, 576, 256)];
        let banks = [bank(archive, 3, 0x8003_3E00, 2304)];
        let descriptor = (2 << 28) | (5 << 24);
        assert!(terrain_joins(archive, &[descriptor], &sheets, &banks.iter().collect::<Vec<_>>()).is_empty());

        let sheets = [sheet(7, 576, 256)];
        let banks = [bank(archive, 3, 0x8003_3E00, 2304), bank_with(archive, 4, 0x8003_3E00, 2304, 1)];
        assert!(
            terrain_joins(archive, &[descriptor], &sheets, &banks.iter().collect::<Vec<_>>()).is_empty(),
            "two different banks at the same row are ambiguous"
        );
    }

    #[test]
    fn a_terrain_join_records_the_sheet_and_the_row() {
        let archive = "BIN/WORLD00/AREA000.EMI";
        let sheets = [sheet(7, 576, 256)];
        let banks = [bank(archive, 3, 0x8003_3E00, 2304)];
        let descriptor = (2 << 28) | (5 << 24);
        let joins = terrain_joins(archive, &[descriptor, descriptor], &sheets, &banks.iter().collect::<Vec<_>>());
        assert_eq!(joins.len(), 1, "a repeated descriptor is one row");
        assert_eq!(joins[0].texture, 7);
        assert_eq!(joins[0].palette.bank.slot, 3);
        assert_eq!((joins[0].clut.x, joins[0].clut.y), (0, 488));
        assert_eq!(joins[0].rule, Rule::Terrain { page_x: 576, page_y: 256 });
    }

    /// The AREA024 site's page and CLUT: a 4bpp page at (704, 256) and row (160, 483).
    fn area024() -> (&'static str, [Sheet; 1], u32) {
        ("BIN/WORLD00/AREA024.EMI", [sheet(7, 704, 256)], 0x8003_3F40)
    }

    #[test]
    fn a_bank_from_another_archive_of_the_family_serves_the_row() {
        let (own, sheets, ram) = area024();
        let other = bank("BIN/WORLD00/AREA099.EMI", 8, ram, 16);
        let joins = joins(own, &sheets, &[&other]);
        assert_eq!(joins.len(), 1);
        assert_eq!(joins[0].palette.archive, "BIN/WORLD00/AREA099.EMI");
        assert_eq!(joins[0].palette.bank.slot, 8);
    }

    #[test]
    fn the_archives_own_bank_wins_over_the_family() {
        let (own, sheets, ram) = area024();
        let own_bank = bank_with(own, 1, ram, 16, 3);
        let other = bank_with("BIN/WORLD00/AREA099.EMI", 8, ram, 16, 9);
        let joins = joins(own, &sheets, &[&own_bank, &other]);
        assert_eq!(joins.len(), 1);
        assert_eq!(joins[0].palette.archive, own);
    }

    #[test]
    fn identical_family_banks_collapse_and_disagreeing_ones_refuse() {
        let (own, sheets, ram) = area024();
        let same =
            [bank_with("BIN/WORLD00/AREA098.EMI", 2, ram, 16, 7), bank_with("BIN/WORLD00/AREA099.EMI", 3, ram, 16, 7)];
        assert_eq!(joins(own, &sheets, &same.iter().collect::<Vec<_>>()).len(), 1);

        let different =
            [bank_with("BIN/WORLD00/AREA098.EMI", 2, ram, 16, 7), bank_with("BIN/WORLD00/AREA099.EMI", 3, ram, 16, 8)];
        assert!(joins(own, &sheets, &different.iter().collect::<Vec<_>>()).is_empty());
    }
}
