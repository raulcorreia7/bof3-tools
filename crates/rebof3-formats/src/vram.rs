//! The VRAM texture upload (entry type 3): rectangles of 16-bit words the loader copies into the
//! frame buffer (`wiki: formats/graphics#chunk-layout`).

use psx_formats::gfx::texture::{self, Clut, Depth, texel_indices};
use psx_formats::gfx::{clut, png, tim};
use psx_formats::vram::{self, Descriptor};
use rebof3_emi::Entry;

use crate::error::{Error, Result};
use crate::identity::difference;
use crate::registry::{Derived, Description, Format, RoundTrip, Selector, State};

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
    kind: "texture",
};

/// Decodes the upload and encodes it again; `Err` names the first difference.
fn round_trip(entry: &Entry, payload: &[u8]) -> Result<(), String> {
    let rectangles =
        vram::decode(payload, Descriptor::from_load_argument(entry.ram_ptr)).map_err(|error| error.to_string())?;
    difference(&vram::encode(&rectangles), payload)
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
    /// The EMI slot of the palette bank that holds the row.
    pub palette: usize,
    /// The disc path of the archive that loads the bank; the texture's own archive,
    /// unless the row came from another archive of the resident set.
    pub palette_archive: String,
    /// The row's VRAM position.
    pub clut: Clut,
    /// The texel depth the draw site's texture page selects.
    pub depth: Depth,
    /// The row's colors as RGBA (see [`color_rgba`]).
    pub colors: Vec<[u8; 4]>,
    /// The row's color words, as the bank stores them.
    pub words: Vec<u16>,
    /// The source lines that join the row to the texture.
    pub evidence: String,
}

/// A texture read at one palette row: its texel indices and the row's colors.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexedImage {
    /// The EMI slot of the palette bank that holds the row.
    pub palette: usize,
    /// The disc path of the archive that loads the bank.
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
