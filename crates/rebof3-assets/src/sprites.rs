//! Character sprites: their palettes as JSON, and each section as a PNG sprite sheet with Aseprite JSON.
//!
//! The format lives in `rebof3_formats::sprite` (frame sheets, animations, records and the draw) and
//! `rebof3_formats::sprite_palettes` (the palette JSON). This module is what the `bof3` command and the
//! editor share: it takes an entry payload as the disc stores it, raw or LZ77, and gives it back in the
//! same form, so the two agree on every byte.
//!
//! # Cross-reference
//!
//! | Family | C record (`decomp/include`) |
//! | --- | --- |
//! | character sprites | `Actor` (`include/sys/actor.h`): `+0x4C` section, `+0x50` steps, `+0x54` frame table; `SpriteCell` (`src/engine/slus/slus.h`) |
//!
//! The layout is in `wiki: formats/character`.

use rebof3_emi::lz77;
use rebof3_formats::sprite;

use crate::Result;

pub use rebof3_formats::sprite::{Sheet, SpriteSizes};
pub use rebof3_formats::sprite_palettes::{CharacterSprites, parse};

/// The JSON of the palettes of one character sprites. `payload` is the decoded sprites: a whole container, or one section.
pub fn character_sprites(payload: &[u8]) -> Result<Vec<u8>> {
    Ok(rebof3_formats::sprite_palettes::export(payload)?)
}

/// How an entry stores its sprites.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Packing {
    /// A type-0 entry: the character sprites' bytes.
    Raw,
    /// A type-1 entry: the sprites packed with the loader's LZ77 codec.
    Lz77,
}

/// The sprites bytes of a stored payload, and how it was stored. A payload that reads as character sprites is raw;
/// otherwise it must unpack to one.
pub fn unpack(payload: &[u8]) -> Result<(Packing, Vec<u8>)> {
    if sprite::decode(payload).is_ok() {
        return Ok((Packing::Raw, payload.to_vec()));
    }
    let sprites = lz77::decode(payload)?;
    sprite::decode(&sprites)?;
    Ok((Packing::Lz77, sprites))
}

/// The payload that stores `sprites` the way `original` stored its sprites: `original` itself while the sprites
/// is unchanged, so an LZ77 entry keeps its stored stream.
pub fn pack(original: &[u8], sprites: &[u8]) -> Result<Vec<u8>> {
    let (packing, stored) = unpack(original)?;
    if stored == sprites {
        return Ok(original.to_vec());
    }
    Ok(match packing {
        Packing::Raw => sprites.to_vec(),
        Packing::Lz77 => lz77::encode(sprites),
    })
}

/// The PNG file name a sheet's JSON names.
pub fn image_name(json: &[u8]) -> Result<String> {
    Ok(sprite::image_name(json)?)
}

/// The cell sizes `g_SpriteSizes` holds in the executable (`SLUS_004.22`).
pub fn sprite_sizes(executable: &[u8]) -> Result<SpriteSizes> {
    Ok(SpriteSizes::from_executable(executable)?)
}

/// Section `section` of the stored `payload` as a sprite sheet drawn with palette `palette`. `image` is the
/// PNG's file name, which the JSON names.
pub fn sprite_sheet(payload: &[u8], section: usize, palette: usize, sizes: &SpriteSizes, image: &str) -> Result<Sheet> {
    let sprites = sprite::decode(&unpack(payload)?.1)?;
    Ok(sprite::export(&sprites, section, palette, sizes, image)?)
}

/// The stored payload with a sprite sheet read back into the section it names; see
/// `rebof3_formats::sprite::import` for what changes.
pub fn import_sprite_sheet(json: &[u8], png: &[u8], payload: &[u8], sizes: &SpriteSizes) -> Result<Vec<u8>> {
    let sprites = sprite::decode(&unpack(payload)?.1)?;
    let imported = sprite::encode(&sprite::import(&sprites, sizes, json, png)?)?;
    pack(payload, &imported)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rebof3_formats::sprite::{Animation, Cell, Encoding, Frame, Image, SHAPES, Step};

    /// A bare section: one sheet, one palette, one animation of one frame, one record, and padding.
    fn section() -> Vec<u8> {
        let texels: Vec<u8> = (0..16 * 8).map(|index| (index % 31 + 1) as u8).collect();
        let sheet = Image::new(8, 8, Encoding::Raw, texels).expect("a sheet").encode();
        let animation = Animation { steps: vec![Step { delay: 3, frame: 0 }], frames: vec![Frame { cells: vec![Cell { size: 1, x: -8, y: -4, u: 0, v: 0 }] }] }.encode();
        let mut out = vec![0u8; 16];
        let images = out.len();
        out.extend(4u32.to_le_bytes());
        out.extend(&sheet);
        let palettes = out.len();
        out.extend(4u32.to_le_bytes());
        out.extend((0..32u16).flat_map(|colour| (colour * 0x0421).to_le_bytes()));
        let animations = out.len();
        out.extend(2u16.to_le_bytes());
        out.extend(&animation);
        let records = out.len();
        out.extend([0, 0, 0, 0xFF]);
        out.resize(out.len() + 0x200, 0);
        for (index, at) in [images, palettes, animations, records].into_iter().enumerate() {
            out[4 * index..4 * index + 4].copy_from_slice(&(at as u32).to_le_bytes());
        }
        out
    }

    fn sizes() -> SpriteSizes {
        let mut rows = [[0; 2]; SHAPES];
        for (shape, row) in rows.iter_mut().enumerate() {
            *row = [8 * (shape as u8 % 4 + 1), 8 * (shape as u8 / 4 + 1)];
        }
        SpriteSizes::from_rows(rows)
    }

    #[test]
    fn a_stored_payload_unpacks_and_packs_to_itself() {
        let raw = section();
        assert_eq!(unpack(&raw).expect("raw").0, Packing::Raw);
        let packed = lz77::encode(&raw);
        let (packing, sprites) = unpack(&packed).expect("LZ77");
        assert_eq!((packing, &sprites), (Packing::Lz77, &raw));
        assert_eq!(pack(&packed, &sprites).expect("packs"), packed, "an unchanged sprites keeps its stream");
        let mut edited = sprites.clone();
        edited[0x30] ^= 1;
        assert_eq!(lz77::decode(&pack(&packed, &edited).expect("packs")).expect("decodes"), edited);
    }

    #[test]
    fn an_unchanged_sheet_imports_to_the_stored_payload() {
        let packed = lz77::encode(&section());
        let sheet = sprite_sheet(&packed, 0, 0, &sizes(), "sprites.png").expect("exports");
        assert_eq!(import_sprite_sheet(&sheet.json, &sheet.png, &packed, &sizes()).expect("imports"), packed);
    }

    #[test]
    fn a_payload_that_is_no_sprite_container_is_refused() {
        assert!(unpack(&[1, 2, 3, 4, 5, 6, 7, 8]).is_err());
    }
}
