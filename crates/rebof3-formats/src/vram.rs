//! The VRAM texture upload (entry type 3): rectangles of 16-bit words the loader copies into the
//! frame buffer (`wiki: formats/graphics#chunk-layout`).

use psx_formats::gfx::texture::{self, Clut, Depth, texel_indices};
use psx_formats::gfx::{clut, png, tim};
use psx_formats::vram::{self, Descriptor};
use rebof3_emi::Entry;

use crate::error::{Error, Result};
use crate::identity::difference;
use crate::registry::{Derived, Description, Extra, Extras, Format, RoundTrip, Selector, Site, State};

/// The upload as a format-registry row.
pub static UPLOAD: Description = Description {
    format: Format::VramUpload,
    name: "VRAM texture upload (type 3)",
    state: State::Codec,
    spec_state: "documented",
    codec_state: "decode+encode",
    evidence: "wiki: formats/graphics#chunk-layout; descriptor from the load argument",
    extension: "img",
    selector: Selector::Type(vram::UPLOAD_TYPE),
    round_trip: RoundTrip::Payload(round_trip),
    derive: Some(derive),
    open: Some(open),
    kind: "texture",
};

/// Decodes the upload and encodes it again; `Err` names the first difference.
fn round_trip(entry: &Entry, payload: &[u8]) -> Result<(), String> {
    let rectangles =
        vram::decode(payload, Descriptor::from_load_argument(entry.ram_ptr)).map_err(|error| error.to_string())?;
    difference(&vram::encode(&rectangles), payload)
}

/// The upload as one indexed PNG per palette row it is drawn with, and the TIM of the same row.
fn open(site: &Site<'_>) -> Extras {
    let mut extras = Extras::default();
    if site.rows.is_empty() {
        // No rule joins the texture to a palette row, so the TIM carries no CLUT: guessing one would
        // put a placement in the header that the game never states.
        match texture_tim_direct(site.entry().ram_ptr, site.payload()) {
            Ok(data) => extras.files.push(Extra {
                role: "tim",
                tag: String::new(),
                extension: "tim",
                evidence: "the upload's VRAM words as a 15-bit TIM; no palette row is joined".to_string(),
                data,
            }),
            Err(error) => extras.skipped = Some(format!("tim skipped: {error}")),
        }
    }
    match indexed_rows(site.entry().ram_ptr, site.payload(), site.rows) {
        Ok(files) => {
            for file in files {
                extras.files.push(Extra {
                    role: "png",
                    tag: file.tag.clone(),
                    extension: "png",
                    evidence: file.evidence.clone(),
                    data: file.data,
                });
                if let Some(tim) = file.tim {
                    extras.files.push(Extra {
                        role: "tim",
                        tag: file.tag,
                        extension: "tim",
                        evidence: file.evidence,
                        data: tim,
                    });
                }
            }
        }
        Err(error) => extras.skipped = Some(format!("palette rows skipped: {error}")),
    }
    extras
}

/// The 16 bpp PNG view of the upload.
fn derive(load_arg: u32, payload: &[u8]) -> Result<Vec<Derived>> {
    let image = vram::upload_image(payload, Descriptor::from_load_argument(load_arg))?;
    let data = psx_formats::gfx::png::rgba_png(image.width, image.height, &image.rgba);
    Ok(vec![Derived { role: "png", extension: "png", data }])
}

/// A palette row a texture is drawn with, joined from a draw site.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RowUse {
    /// The EMI slot of the CLUT block that holds the row.
    pub palette: usize,
    /// The disc path of the archive that loads the block; the texture's own archive,
    /// unless the row came from another archive of the resident set.
    pub palette_archive: String,
    /// The row's VRAM position.
    pub clut: Clut,
    /// The texel depth the draw site's texture page selects.
    pub depth: Depth,
    /// The row's colors as RGBA (see [`color_rgba`]).
    pub colors: Vec<[u8; 4]>,
    /// The row's color words, as the block stores them.
    pub words: Vec<u16>,
    /// The source lines that join the row to the texture.
    pub evidence: String,
}

/// A texture read at one palette row: its texel indices and the row's colors.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexedImage {
    /// The EMI slot of the CLUT block that holds the row.
    pub palette: usize,
    /// The disc path of the archive that loads the block.
    pub palette_archive: String,
    /// The row's VRAM position.
    pub clut: Clut,
    /// The texel depth.
    pub depth: Depth,
    /// Width in pixels.
    pub width: usize,
    /// Height in pixels.
    pub height: usize,
    /// `width * height` texel indices, row-major.
    pub indices: Vec<u8>,
    /// The row's colors as RGBA.
    pub colors: Vec<[u8; 4]>,
    /// The source lines that join the row to the texture.
    pub evidence: String,
}

/// An indexed PNG of a texture read in one palette row.
pub struct RowFile {
    /// Text between the slot and the extension: `.clut-160-483`.
    pub tag: String,
    /// The draw site that joins the row to the texture.
    pub evidence: String,
    /// The PNG bytes.
    pub data: Vec<u8>,
    /// The standard PS1 TIM file of the same row: the texture's VRAM words as
    /// the image and the row's color words as the CLUT.
    pub tim: Option<Vec<u8>>,
}

/// A texture upload's words: the VRAM position of its top-left word and its row-major grid.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextureWords {
    /// The left edge in VRAM words.
    pub x: u32,
    /// The top edge in VRAM rows.
    pub y: u32,
    /// Width in VRAM words.
    pub width: usize,
    /// Height in VRAM rows.
    pub height: usize,
    /// `width * height` words, row-major.
    pub words: Vec<u16>,
}

/// The words of a texture upload: where its rectangles land in VRAM and their row-major grid.
pub fn texture_words(load_arg: u32, payload: &[u8]) -> Result<TextureWords> {
    let descriptor = vram::Descriptor::from_load_argument(load_arg);
    let grid = vram::word_grid(payload, descriptor)?;
    let side = vram::RECTANGLE_SIDE as u32;
    Ok(TextureWords {
        x: u32::from(descriptor.column) * side,
        y: u32::from(descriptor.row) * side,
        width: grid.width,
        height: grid.height,
        words: grid.words,
    })
}

/// The standard PS1 TIM file of a texture read in one palette row: the
/// texture's VRAM words as the image and the row's color words as the CLUT.
///
/// The game stores no TIM, but a texture and its palette are what a TIM
/// holds, so the file is exact: the words are the words the disc stores.
pub fn texture_tim(load_arg: u32, payload: &[u8], row: &RowUse) -> Result<Vec<u8>> {
    texture_tim_of(load_arg, payload, row.depth.bits(), &row.words, row.clut.x, row.clut.y)
}

/// The standard PS1 TIM file of a texture drawn with one CLUT row: `bits` is
/// 4 or 8, `words` is the row's color words, and `clut_x` and `clut_y` are the
/// row's place in the frame buffer.
pub fn texture_tim_of(
    load_arg: u32,
    payload: &[u8],
    bits: u32,
    words: &[u16],
    clut_x: u16,
    clut_y: u16,
) -> Result<Vec<u8>> {
    let descriptor = vram::Descriptor::from_load_argument(load_arg);
    let grid = vram::word_grid(payload, descriptor)?;
    let mode = match bits {
        4 => tim::Mode::Four,
        8 => tim::Mode::Eight,
        other => return Err(Error::Import(format!("a texture is 4- or 8-bit, not {other}-bit"))),
    };
    let side = u16::try_from(vram::RECTANGLE_SIDE).unwrap_or(u16::MAX);
    let tim = tim::File {
        mode,
        clut: Some(tim::Block {
            x: clut_x,
            y: clut_y,
            width: u16::try_from(words.len()).unwrap_or(u16::MAX),
            height: 1,
            words: words.to_vec(),
        }),
        image: tim::Block {
            x: u16::from(descriptor.column).saturating_mul(side),
            y: u16::from(descriptor.row).saturating_mul(side),
            width: u16::try_from(grid.width).unwrap_or(u16::MAX),
            height: u16::try_from(grid.height).unwrap_or(u16::MAX),
            words: grid.words,
        },
    };
    Ok(tim.write())
}

/// The TIM of a texture with no CLUT: the upload's VRAM words as a 15-bit image at the place its load
/// argument gives (Sony *File Formats*, chapter 3, "TIM": the pixel mode field `2` has no CLUT block).
///
/// This is the file for a texture no rule joins to a palette row: the header names no CLUT, because the
/// game states none. The words are the words the disc stores, so [`import_tim`] returns the payload.
pub fn texture_tim_direct(load_arg: u32, payload: &[u8]) -> Result<Vec<u8>> {
    let descriptor = vram::Descriptor::from_load_argument(load_arg);
    let grid = vram::word_grid(payload, descriptor)?;
    let place = Placement::of(descriptor);
    let tim = tim::File {
        mode: tim::Mode::Fifteen,
        clut: None,
        image: tim::Block {
            x: place.x(),
            y: place.y(),
            width: u16::try_from(grid.width).unwrap_or(u16::MAX),
            height: u16::try_from(grid.height).unwrap_or(u16::MAX),
            words: grid.words,
        },
    };
    Ok(tim.write())
}

/// Where a type-3 upload lands: the three fields of its load argument (`wiki: formats/graphics`). The
/// upload is whole 32-by-32-word rectangles on a 32-word, 32-row grid, `chunks_per_row` of them to a row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Placement {
    /// The first rectangle's column, in 32-word units (6 bits; VRAM holds 32).
    pub column: u8,
    /// The first rectangle's row, in 32-row units (5 bits; VRAM holds 16).
    pub row: u8,
    /// Rectangles per row.
    pub chunks_per_row: u8,
}

/// The bits of a load argument the placement owns: column, row and rectangles per row.
const PLACEMENT_BITS: u32 = 0x3f1f_3f00;

impl Placement {
    /// The placement a load argument names.
    pub fn of(descriptor: vram::Descriptor) -> Self {
        Self { column: descriptor.column, row: descriptor.row, chunks_per_row: descriptor.chunks_per_row }
    }

    /// The placement a load argument names.
    pub fn of_load_argument(load_arg: u32) -> Self {
        Self::of(vram::Descriptor::from_load_argument(load_arg))
    }

    /// The left edge in VRAM words.
    pub fn x(self) -> u16 {
        u16::from(self.column) * vram::RECTANGLE_SIDE as u16
    }

    /// The top edge in VRAM rows.
    pub fn y(self) -> u16 {
        u16::from(self.row) * vram::RECTANGLE_SIDE as u16
    }

    /// `base` with this placement in the bits it owns; every other bit of `base` stays.
    pub fn load_argument(self, base: u32) -> u32 {
        base & !PLACEMENT_BITS
            | u32::from(self.column) << 24
            | u32::from(self.row) << 16
            | u32::from(self.chunks_per_row) << 8
    }

    /// The same upload with its top-left rectangle at VRAM `column` and `row` (32-word and 32-row units),
    /// or `None` when `rectangles` of it would leave the frame buffer.
    pub fn moved(self, column: u8, row: u8, rectangles: usize) -> Option<Self> {
        let per_row = usize::from(self.chunks_per_row).max(1);
        let rows = rectangles.div_ceil(per_row);
        let fits = usize::from(column) + per_row <= vram::VRAM_WIDTH / vram::RECTANGLE_SIDE
            && usize::from(row) + rows <= vram::VRAM_HEIGHT / vram::RECTANGLE_SIDE;
        fits.then_some(Self { column, row, ..self })
    }
}

/// The CLUT block of a TIM: its place in VRAM and its color words.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimClut {
    /// The left edge in VRAM words.
    pub x: u16,
    /// The top edge in VRAM rows.
    pub y: u16,
    /// Words per CLUT row (16 for 4 bpp, 256 for 8 bpp).
    pub width: u16,
    /// CLUT rows.
    pub height: u16,
    /// `width * height` color words, row-major.
    pub words: Vec<u16>,
}

/// A TIM read as a type-3 texture: the payload its image makes and the placement its header states.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimTexture {
    /// The type-3 payload.
    pub payload: Vec<u8>,
    /// The texel depth, or `None` for a 15-bit image.
    pub depth: Option<Depth>,
    /// The upload's place, from the image block's VRAM coordinates and width.
    pub placement: Placement,
    /// The image block's height in rows.
    pub height: u16,
    /// The CLUT block, when the file has one.
    pub clut: Option<TimClut>,
}

/// Reads a TIM file as a texture upload (Sony *File Formats*, chapter 3, "TIM").
///
/// The image block's words are the upload's words, so a TIM that [`texture_tim`] or [`texture_tim_direct`]
/// wrote returns the payload byte for byte. The game uploads whole 32-by-32-word rectangles on a 32-word,
/// 32-row grid; a TIM whose image is not that shape is refused with its size, because padding it would put
/// words in the texture that the TIM does not hold. An image that leaves the 1024-by-512 frame buffer is
/// kept, as the disc keeps it: the loader skips those rectangles.
pub fn import_tim(bytes: &[u8]) -> Result<TimTexture> {
    let file = tim::File::read(bytes)?;
    let image = &file.image;
    let side = vram::RECTANGLE_SIDE;
    let (x, y) = (usize::from(image.x), usize::from(image.y));
    let (width, height) = (usize::from(image.width), usize::from(image.height));
    let shaped = [x, y, width, height].iter().all(|value| value % side == 0) && width != 0 && height != 0;
    if !shaped {
        return Err(Error::Import(format!(
            "the TIM image is {width} by {height} words at ({x}, {y}); a texture upload is whole {side} by {side}              word rectangles on a {side}-word grid, so size and place the image to multiples of {side}"
        )));
    }
    // The loader skips the rectangles of a retail upload that leave the frame buffer, so the TIM keeps
    // them: only what the load argument cannot name is refused.
    if x / side > 0x3f || y / side > 0x1f {
        return Err(Error::Import(format!(
            "the TIM image at ({x}, {y}) is past the place a load argument can name (column 63, row 31)"
        )));
    }
    let chunks_per_row = u8::try_from(width / side)
        .ok()
        .filter(|per_row| *per_row <= 0x3f)
        .ok_or_else(|| Error::Import(format!("{width} words per row is more than a load argument can name")))?;
    let placement = Placement { column: (x / side) as u8, row: (y / side) as u8, chunks_per_row };
    let payload = vram::payload_from_grid(width, height, &image.words)?;
    let depth = match file.mode {
        tim::Mode::Four => Some(Depth::Four),
        tim::Mode::Eight => Some(Depth::Eight),
        tim::Mode::Fifteen => None,
    };
    let clut = file.clut.map(|block| TimClut {
        x: block.x,
        y: block.y,
        width: block.width,
        height: block.height,
        words: block.words,
    });
    Ok(TimTexture { payload, depth, placement, height: image.height, clut })
}

/// The texture of an upload read at each palette row in `rows`: its texel indices at the row's depth.
pub fn indexed_images(load_arg: u32, payload: &[u8], rows: &[RowUse]) -> Result<Vec<IndexedImage>> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let grid = vram::word_grid(payload, vram::Descriptor::from_load_argument(load_arg))?;
    Ok(rows
        .iter()
        .map(|row| IndexedImage {
            palette: row.palette,
            palette_archive: row.palette_archive.clone(),
            clut: row.clut,
            depth: row.depth,
            width: grid.width * row.depth.texels_per_word(),
            height: grid.height,
            indices: texel_indices(&grid.words, row.depth),
            colors: row.colors.clone(),
            evidence: row.evidence.clone(),
        })
        .collect())
}

/// The 16 bpp view of a texture upload, as the export writes it (`<slot>.png`), from the payload and
/// its load argument.
pub fn texture_view_png(load_arg: u32, payload: &[u8]) -> Result<Vec<u8>> {
    let image = vram::upload_image(payload, vram::Descriptor::from_load_argument(load_arg))?;
    Ok(png::rgba_png(image.width, image.height, &image.rgba))
}

/// The indexed PNG of a texture upload drawn with one palette row: `bits` is 4 or 8, and `palette` holds
/// the row's `2^bits` color words. This is the export of [`indexed_rows`] for a row given by its words.
pub fn texture_row_png(load_arg: u32, payload: &[u8], bits: u32, palette: &[u16]) -> Result<Vec<u8>> {
    let depth = match bits {
        4 => Depth::Four,
        8 => Depth::Eight,
        other => return Err(Error::Import(format!("a texture is 4- or 8-bit indexed, not {other}-bit"))),
    };
    if palette.len() != depth.colors() {
        return Err(Error::Import(format!(
            "a {bits}-bit row holds {} color words, not {}",
            depth.colors(),
            palette.len()
        )));
    }
    let grid = vram::word_grid(payload, vram::Descriptor::from_load_argument(load_arg))?;
    let indices = texel_indices(&grid.words, depth);
    let colors: Vec<[u8; 4]> = palette.iter().map(|&word| clut::word_to_view(word)).collect();
    let width = grid.width * depth.texels_per_word();
    Ok(png::indexed_png(width as u32, grid.height as u32, bits as u8, &colors, &indices))
}

/// One indexed PNG per palette row: the upload's texels at the row's depth, in the row's colors.
pub fn indexed_rows(load_arg: u32, payload: &[u8], rows: &[RowUse]) -> Result<Vec<RowFile>> {
    Ok(indexed_images(load_arg, payload, rows)?
        .into_iter()
        .map(|image| {
            let tag = format!(".clut-{}-{}", image.clut.x, image.clut.y);
            let tim = rows
                .iter()
                .find(|row| row.clut == image.clut && row.depth == image.depth)
                .and_then(|row| texture_tim(load_arg, payload, row).ok());
            RowFile {
                tag,
                data: png::indexed_png(
                    image.width as u32,
                    image.height as u32,
                    image.depth.bits() as u8,
                    &image.colors,
                    &image.indices,
                ),
                evidence: image.evidence,
                tim,
            }
        })
        .collect())
}

/// A texture read back from an indexed row: its payload and the CLUT row its
/// texels are drawn with.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexedTexture {
    /// The type-3 payload, as stored.
    pub payload: Vec<u8>,
    /// The CLUT row's color words, as stored.
    pub palette: Vec<u16>,
    /// The texel depth, from the PNG's bit depth.
    pub depth: Depth,
}

/// What a texture PNG holds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TextureImport {
    /// A 16 bpp view: the payload, as stored.
    Words(Vec<u8>),
    /// An indexed row: the payload and the CLUT row it is drawn with.
    Indexed(IndexedTexture),
}

/// Reads a texture PNG that the export wrote, as its stored payload.
///
/// A view returns every word exactly, STP included. An indexed row packs its
/// texels back into words of its depth, and its colors return as color words.
pub fn import(bytes: &[u8]) -> Result<TextureImport> {
    Ok(match png::read(bytes)? {
        png::Picture::Rgba(picture) => {
            let words: Vec<u16> =
                picture.pixels.as_chunks::<4>().0.iter().map(|&pixel| clut::view_to_word(pixel)).collect();
            let payload = vram::payload_from_grid(picture.width as usize, picture.height as usize, &words)?;
            TextureImport::Words(payload)
        }
        png::Picture::Indexed(picture) => {
            let depth = if picture.bits == 4 { Depth::Four } else { Depth::Eight };
            let (width, height) = (picture.width as usize, picture.height as usize);
            let per_word = depth.texels_per_word();
            if width % per_word != 0 {
                let words = picture.indices.len();
                return Err(psx_formats::Error::VramGridShape { width, height, words }.into());
            }
            let words = texture::texel_words(&picture.indices, depth);
            let payload = vram::payload_from_grid(width / per_word, height, &words)?;
            let palette = picture.colors.iter().map(|&color| clut::view_to_word(color)).collect();
            TextureImport::Indexed(IndexedTexture { payload, palette, depth })
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A two-rectangle sheet at column 22, row 8, one rectangle to a row, with distinct words.
    const LOAD: u32 = (22 << 24) | (8 << 16) | (1 << 8);

    fn sheet() -> Vec<u8> {
        (0..2 * 1024u16).flat_map(|word| word.wrapping_mul(7).to_le_bytes()).collect()
    }

    fn row(depth: u32) -> Vec<u16> {
        (0..(1u16 << depth)).map(|word| 0x8000 | word).collect()
    }

    #[test]
    fn a_joined_texture_goes_through_a_tim_and_back() {
        let tim = texture_tim_of(LOAD, &sheet(), 4, &row(4), 160, 483).unwrap();
        let file = tim::File::read(&tim).unwrap();
        assert_eq!(file.mode, tim::Mode::Four);
        assert_eq!((file.image.x, file.image.y, file.image.width, file.image.height), (704, 256, 32, 64));
        let clut = file.clut.as_ref().expect("a CLUT block");
        assert_eq!((clut.x, clut.y, clut.width, clut.height), (160, 483, 16, 1));

        let back = import_tim(&tim).unwrap();
        assert_eq!(back.payload, sheet());
        assert_eq!(back.depth, Some(Depth::Four));
        assert_eq!(back.placement.load_argument(LOAD), LOAD);
        assert_eq!(back.clut.expect("the CLUT").words, row(4));
    }

    #[test]
    fn an_unjoined_texture_has_no_clut() {
        let tim = texture_tim_direct(LOAD, &sheet()).unwrap();
        let file = tim::File::read(&tim).unwrap();
        assert_eq!((file.mode, file.clut), (tim::Mode::Fifteen, None));
        let back = import_tim(&tim).unwrap();
        assert_eq!((back.payload, back.depth, back.clut), (sheet(), None, None));
    }

    #[test]
    fn a_moved_image_changes_only_the_placement_bits() {
        let base = LOAD | 0x00c0_00ff;
        let mut file = tim::File::read(&texture_tim_direct(base, &sheet()).unwrap()).unwrap();
        file.image.x = 64;
        file.image.y = 32;
        let back = import_tim(&file.write()).unwrap();
        assert_eq!(back.placement.load_argument(base), 0x0201_0100 | 0x00c0_00ff);
    }

    #[test]
    fn an_image_off_the_upload_grid_is_refused() {
        let mut file = tim::File::read(&texture_tim_direct(LOAD, &sheet()).unwrap()).unwrap();
        file.image.x += 1;
        assert!(import_tim(&file.write()).unwrap_err().to_string().contains("multiples of 32"));
        let mut small = tim::File::read(&texture_tim_direct(LOAD, &sheet()).unwrap()).unwrap();
        small.image = tim::Block { width: 16, height: 16, words: vec![0; 256], ..small.image };
        assert!(import_tim(&small.write()).is_err());
    }

    #[test]
    fn a_move_stays_in_the_frame_buffer() {
        let place = Placement::of_load_argument(LOAD);
        assert_eq!(place.moved(30, 14, 2).map(|moved| (moved.column, moved.row)), Some((30, 14)));
        assert_eq!(place.moved(32, 14, 2), None, "column 32 is past the right edge");
        assert_eq!(place.moved(30, 15, 2), None, "two rows from row 15 leave the bottom");
    }
}
