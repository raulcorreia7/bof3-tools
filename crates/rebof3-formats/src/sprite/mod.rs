//! Character sprites: the character and enemy sprites the field loads to `0x8003B800`.
//!
//! Character sprites are a container of three sections (its first word is [`CONTAINER_HEAD`]: `BPLCHAR`, `PLCHAR`
//! and `ETC/START.EMI` entry 7), or one section by itself (the `BPLCHAR` entries at `0x8003B80C` and the
//! other section positions). `Party_LoadMemberSprite` (`emi/etc/game/00@0x801C1254`) points an actor's work
//! `+0x4C` at section `n` (`0x8003B800 + header[n]`), and the readers below take its four header words:
//!
//! | Offset | Field | Reader | Meaning |
//! | -----: | ----- | ------ | ------- |
//! | `0x00` | `images` | `Actor_SelectScriptBank` (`header[0]`) | offset of the frame-sheet table: one word offset per [`Image`], from the table |
//! | `0x04` | `palettes` | `GFX_LoadPalette` (`record + 4`) | offset of the palette table: one word offset per 32-colour palette, from the table |
//! | `0x08` | `animations` | `Actor_SelectScriptBank` (`header[2]`) | offset of the animation table: one halfword offset per [`Animation`], from the table |
//! | `0x0C` | `records` | `Actor_SelectScriptBank` (`header[3]`) | offset of the 3-byte [`Record`]s |
//!
//! `Actor_SelectScriptBank` (`exe/slus_004_22@0x8014DC08`) takes record `n` (the actor's script record,
//! work `+0x4B`): it queues sheet `record[0]` for upload, points work `+0x50` at animation `record[1]`
//! and stores `record[2]` in work `+0x2A`, which `Area_DrawObjectSprite` reads as "mirror the actor".
//! `GFX_LoadPalette` (`exe/slus_004_22@0x80166C48`) copies one palette; `Party_LoadMemberSprite` asks for
//! palette 0.
//!
//! In every retail section the parts follow the header in this order: the sheet table and the sheets,
//! the palette table and the palettes, the animation table and the animations, and the records. No word
//! stores how many records there are: the table runs while each record names a sheet and an animation
//! of the section and a mirror flag of 0 or 1, and the zero bytes that end a section are padding.
//!
//! [`decode`] reads character sprites and [`encode`] writes it back; with nothing changed the bytes are the stored
//! ones. An edit is written in place: palettes, steps, cells and records keep their size. An edited
//! sheet is packed again, and the parts after it move; the section keeps its length, so an edit that
//! needs more than the section's trailing zero bytes is refused.

mod draw;
mod image;
mod sheet;

pub use crate::animation::{
    Animation, CELL_BYTES, Cell, FLIP, Frame, JUMP, Playback, Player, SHAPE_MASK, Step, TICK_MICROS, duration_ms,
    ticks_of,
};
pub use draw::{
    Bounds, Composite, FIELD_PAGE, FIELD_PALETTE, FIELD_PALETTE_STAGE, FIELD_SLOT_ROWS, Pixel, Placement, Reader,
    SHAPES, SPRITE_SIZES, SpriteSizes, Stage, TRANSPARENT, Texel, animation_bounds, cell_rect, compose, frame_bounds,
};
pub use image::{Encoding, Image, PACKED_MAX, TEXELS_PER_WORD};
pub use sheet::{Sheet, export, export_section, image_name, import, import_section};

use crate::error::{Error, Result};

/// The first word of a container: the offset of its first section, which is also the header's size.
pub const CONTAINER_HEAD: u32 = 0x0C;
/// Sections in a container.
pub const CONTAINER_SECTIONS: usize = 3;
/// 16-bit colours in one palette (`GFX_LoadPalette` copies 32).
pub const COLOURS: usize = 0x20;
/// Bytes in one palette.
pub const PALETTE_BYTES: usize = COLOURS * 2;
/// Bytes in one record.
pub const RECORD_BYTES: usize = 3;
/// Header words of a section.
const HEADER_WORDS: usize = 4;
const WORD: usize = 4;
const HALF: usize = 2;
/// A record's mirror flag is 0 or 1 in every retail record.
const MIRROR_MAX: u8 = 1;

/// One palette: 16-bit colours in CLUT order.
pub type Palette = [u16; COLOURS];

/// One record: the sheet, the animation and the mirror flag an actor's script record selects.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Record {
    pub image: u8,
    pub animation: u8,
    pub mirror: u8,
}

/// Where a section's parts sit in its stored bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Layout {
    /// The header words: the sheet, palette, animation and record table offsets.
    header: [usize; HEADER_WORDS],
    /// The offset of each sheet record.
    images: Vec<usize>,
    /// The offset of each palette.
    palettes: Vec<usize>,
    /// The offset and the stored length of each animation.
    animations: Vec<(usize, usize)>,
    /// The bytes the section holds, and the end of the bytes that are not trailing padding.
    len: usize,
    used: usize,
}

/// One section: its sheets, palettes, animations and records.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Section {
    pub images: Vec<Image>,
    pub palettes: Vec<Palette>,
    pub animations: Vec<Animation>,
    pub records: Vec<Record>,
    stored: Vec<u8>,
    layout: Layout,
}

/// The sections of character sprites, in container order. A bare section is a container of one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CharacterSprites {
    pub sections: Vec<Section>,
    /// Whether the character sprites open with the three-word container header.
    pub container: bool,
}

impl Section {
    /// The section in `bytes`.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut header = [0; HEADER_WORDS];
        for (index, slot) in header.iter_mut().enumerate() {
            *slot = to_usize(word(bytes, WORD * index)?);
        }
        let [images_at, palettes_at, animations_at, records_at] = header;
        if !(HEADER_WORDS * WORD <= images_at
            && images_at < palettes_at
            && palettes_at < animations_at
            && animations_at < records_at
            && records_at <= bytes.len())
        {
            return Err(Error::CharacterSprites(format!(
                "the header offsets {images_at:#X}, {palettes_at:#X}, {animations_at:#X}, {records_at:#X} are not \
                 sheets, palettes, animations and records in order inside {:#X} bytes",
                bytes.len()
            )));
        }

        let image_offsets = table(bytes, images_at, WORD, palettes_at)?;
        let mut images = Vec::with_capacity(image_offsets.len());
        for (index, &start) in image_offsets.iter().enumerate() {
            let end = image_offsets.get(index + 1).copied().unwrap_or(palettes_at);
            if start >= end {
                return Err(Error::CharacterSprites(format!(
                    "frame sheet {index} at {start:#X} is not before {end:#X}"
                )));
            }
            images.push(Image::decode(&bytes[start..end])?);
        }

        let palette_offsets = table(bytes, palettes_at, WORD, animations_at)?;
        let mut palettes = Vec::with_capacity(palette_offsets.len());
        for &at in &palette_offsets {
            if at + PALETTE_BYTES > animations_at {
                return Err(Error::CharacterSprites(format!("the palette at {at:#X} runs into the animations")));
            }
            let mut palette = [0u16; COLOURS];
            for (colour, pair) in palette.iter_mut().zip(bytes[at..at + PALETTE_BYTES].as_chunks::<HALF>().0) {
                *colour = u16::from_le_bytes(*pair);
            }
            palettes.push(palette);
        }

        let animation_offsets = table(bytes, animations_at, HALF, records_at)?;
        let mut animations = Vec::with_capacity(animation_offsets.len());
        let mut spans = Vec::with_capacity(animation_offsets.len());
        for &at in &animation_offsets {
            let (animation, len) = Animation::decode(&bytes[at..records_at])?;
            animations.push(animation);
            spans.push((at, len));
        }

        // The records run to the last byte that is not zero, rounded up to a whole record.
        let nonzero = bytes.iter().rposition(|&byte| byte != 0).map_or(0, |last| last + 1).max(records_at);
        let used = (records_at + (nonzero - records_at).next_multiple_of(RECORD_BYTES)).min(bytes.len());
        let records = bytes[records_at..used]
            .as_chunks::<RECORD_BYTES>()
            .0
            .iter()
            .map(|&[image, animation, mirror]| Record { image, animation, mirror })
            .take_while(|record| {
                usize::from(record.image) < images.len()
                    && usize::from(record.animation) < animations.len()
                    && record.mirror <= MIRROR_MAX
            })
            .collect();

        let layout = Layout {
            header,
            images: image_offsets,
            palettes: palette_offsets,
            animations: spans,
            len: bytes.len(),
            used: used.max(nonzero),
        };
        Ok(Self { images, palettes, animations, records, stored: bytes.to_vec(), layout })
    }

    /// The section's bytes; see the module documentation for what an edit may change.
    pub fn encode(&self) -> Result<Vec<u8>> {
        let [images_at, palettes_at, _, records_at] = self.layout.header;
        if self.images.len() != self.layout.images.len()
            || self.palettes.len() != self.layout.palettes.len()
            || self.animations.len() != self.layout.animations.len()
        {
            return Err(Error::CharacterSprites("a section keeps its sheet, palette and animation counts".into()));
        }

        // The sheet table and the sheets: as stored while every sheet is, otherwise packed again after
        // whatever the stored table has before its first sheet.
        let mut out = self.stored[..images_at].to_vec();
        if self.images.iter().all(Image::is_stored) {
            out.extend(&self.stored[images_at..palettes_at]);
        } else {
            let first = self.layout.images.first().copied().unwrap_or(palettes_at);
            let table_bytes = WORD * self.images.len();
            let mut region = self.stored[images_at + table_bytes..first].to_vec();
            let mut offsets = Vec::with_capacity(self.images.len());
            for image in &self.images {
                offsets.push(table_bytes + region.len());
                region.extend(image.encode());
            }
            for offset in offsets {
                out.extend((offset as u32).to_le_bytes());
            }
            out.extend(region);
        }
        let shift = out.len() as isize - palettes_at as isize;
        out.extend(&self.stored[palettes_at..self.layout.used]);

        let moved = |at: usize| (at as isize + shift) as usize;
        for (index, at) in self.layout.header.iter().enumerate().skip(1) {
            out[WORD * index..WORD * index + WORD].copy_from_slice(&(moved(*at) as u32).to_le_bytes());
        }

        for (palette, &at) in self.palettes.iter().zip(&self.layout.palettes) {
            let at = moved(at);
            for (index, colour) in palette.iter().enumerate() {
                out[at + HALF * index..at + HALF * index + HALF].copy_from_slice(&colour.to_le_bytes());
            }
        }

        for (index, (animation, &(at, stored))) in self.animations.iter().zip(&self.layout.animations).enumerate() {
            let bytes = animation.encode();
            if bytes.len() != stored {
                return Err(Error::CharacterSprites(format!(
                    "animation {index} needs {} bytes; the section keeps its {stored}",
                    bytes.len()
                )));
            }
            let at = moved(at);
            out[at..at + bytes.len()].copy_from_slice(&bytes);
        }

        let records_at = moved(records_at);
        for (index, record) in self.records.iter().enumerate() {
            let at = records_at + RECORD_BYTES * index;
            let slot = out
                .get_mut(at..at + RECORD_BYTES)
                .ok_or_else(|| Error::CharacterSprites(format!("record {index} is past the end of the section")))?;
            slot.copy_from_slice(&[record.image, record.animation, record.mirror]);
        }

        if out.len() > self.layout.len {
            return Err(Error::CharacterSprites(format!(
                "the edited section needs {:#X} bytes; it keeps its {:#X}",
                out.len(),
                self.layout.len
            )));
        }
        out.resize(self.layout.len, 0);
        Ok(out)
    }

    /// The sheet every record that plays `animation` uploads, when the records name one.
    pub fn image_of(&self, animation: usize) -> Option<usize> {
        let mut images = self
            .records
            .iter()
            .filter(|record| usize::from(record.animation) == animation)
            .map(|record| usize::from(record.image));
        let first = images.next()?;
        images.all(|image| image == first).then_some(first)
    }

    /// The bytes the section holds.
    pub fn len(&self) -> usize {
        self.layout.len
    }

    /// Whether the section holds no bytes; a decoded section never does.
    pub fn is_empty(&self) -> bool {
        self.layout.len == 0
    }

    /// The trailing zero bytes an edited sheet may grow into.
    pub fn free(&self) -> usize {
        self.layout.len - self.layout.used
    }
}

/// The character sprites in `payload`: a container when its first word is [`CONTAINER_HEAD`], otherwise one section.
pub fn decode(payload: &[u8]) -> Result<CharacterSprites> {
    if word(payload, 0)? != CONTAINER_HEAD {
        return Ok(CharacterSprites { sections: vec![Section::decode(payload)?], container: false });
    }

    let mut starts = Vec::with_capacity(CONTAINER_SECTIONS);
    for index in 0..CONTAINER_SECTIONS {
        starts.push(to_usize(word(payload, WORD * index)?));
    }
    let mut sections = Vec::with_capacity(CONTAINER_SECTIONS);
    for (index, &start) in starts.iter().enumerate() {
        let end = starts.get(index + 1).copied().unwrap_or(payload.len());
        if start < CONTAINER_HEAD as usize || start >= end || end > payload.len() {
            return Err(Error::CharacterSprites(format!(
                "container section {index} at {start:#X} does not end at {end:#X}"
            )));
        }
        sections.push(Section::decode(&payload[start..end])?);
    }
    Ok(CharacterSprites { sections, container: true })
}

/// The character sprites' bytes: the container header, when it has one, then each section.
pub fn encode(sprites: &CharacterSprites) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    if sprites.container {
        let mut start = CONTAINER_HEAD as usize;
        out.extend(CONTAINER_HEAD.to_le_bytes());
        for section in sprites.sections.iter().take(CONTAINER_SECTIONS - 1) {
            start += section.len();
            out.extend((start as u32).to_le_bytes());
        }
    }
    for section in &sprites.sections {
        out.extend(section.encode()?);
    }
    Ok(out)
}

/// The evidence behind the format, for a view or a report.
pub const EVIDENCE: &str = "Party_LoadMemberSprite (emi/etc/game/00@0x801C1254) picks the section and placement, in the field \
and, through Party_BuildBattlers(PARTY_ORDER_BATTLE) after Battle_LoadPartySprites (emi/etc/game/00@0x80197AA4) streams BPLD or \
BPLU, in battle; \
Actor_SelectScriptBank (exe/slus_004_22@0x8014DC08) reads the header, records, sheets and animations; \
GFX_UpdateTransferQueue, GFX_ExpandRgb555, GFX_DecodeLzss (exe/slus_004_22@0x8014E6D0, 0x8014E7F8, 0x8014E8FC) \
upload a sheet; GFX_LoadPalette (exe/slus_004_22@0x80166C48) stages a palette; Actor_SeekScript, Actor_StageAnimStep, \
Actor_StepAnimLoop, Actor_StepAnimOnce, Actor_StepAnimReverse play the steps; Area_DrawObjectSprite, Area_DrawSpriteCell, \
Area_BuildClut (exe/slus_004_22@0x8014C4D4, 0x8014CF9C, 0x8014CE78) draw the cells with g_SpriteSizes \
(exe/slus_004_22@0x8017F53C)";

/// What the character sprites hold, in the game's terms: one line per section and the evidence.
pub fn describe(sprites: &CharacterSprites) -> String {
    let mut lines = vec![format!(
        "Character sprites: {} {}, loaded to 0x8003B800 and read by the actor sprite code in the field and in battle.",
        sprites.sections.len(),
        if sprites.container { "sections in a container" } else { "section" }
    )];
    for (index, section) in sprites.sections.iter().enumerate() {
        let frames: usize = section.animations.iter().map(|animation| animation.frames.len()).sum();
        let unplaced = (0..section.animations.len()).filter(|&animation| section.image_of(animation).is_none()).count();
        lines.push(format!(
            "Section {index}: {} frame sheets, {} palettes, {} animations with {frames} frames ({unplaced} no record \
             plays), {} records, {} free bytes.",
            section.images.len(),
            section.palettes.len(),
            section.animations.len(),
            section.records.len(),
            section.free()
        ));
    }
    lines.push(format!("Evidence: {EVIDENCE}."));
    lines.join("\n")
}

/// The offsets a table at `at` holds, each made absolute: the first entry is the table's own size, so
/// it gives the count. Every offset must point between the table's end and `end`.
fn table(bytes: &[u8], at: usize, width: usize, end: usize) -> Result<Vec<usize>> {
    let read = |index: usize| -> Result<usize> {
        let position = at + width * index;
        if width == WORD { word(bytes, position).map(to_usize) } else { half(bytes, position) }
    };
    let first = read(0)?;
    if first == 0 || !first.is_multiple_of(width) {
        return Err(Error::CharacterSprites(format!("the table at {at:#X} opens with {first:#X}")));
    }
    let count = first / width;
    let mut offsets = Vec::with_capacity(count);
    for index in 0..count {
        let offset = at + read(index)?;
        if offset < at + first || offset >= end {
            return Err(Error::CharacterSprites(format!(
                "entry {index} of the table at {at:#X} points to {offset:#X}"
            )));
        }
        offsets.push(offset);
    }
    Ok(offsets)
}

fn word(bytes: &[u8], at: usize) -> Result<u32> {
    bytes
        .get(at..at + WORD)
        .map(|quad| u32::from_le_bytes([quad[0], quad[1], quad[2], quad[3]]))
        .ok_or_else(|| Error::CharacterSprites(format!("the character sprites end before the word at {at:#X}")))
}

fn half(bytes: &[u8], at: usize) -> Result<usize> {
    bytes
        .get(at..at + HALF)
        .map(|pair| usize::from(u16::from_le_bytes([pair[0], pair[1]])))
        .ok_or_else(|| Error::CharacterSprites(format!("the character sprites end before the halfword at {at:#X}")))
}

fn to_usize(value: u32) -> usize {
    value as usize
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A section with two sheets (raw and packed), `palettes` palettes, two animations and three records,
    /// then `free` bytes of padding.
    pub(crate) fn section_bytes(palettes: usize, free: usize) -> Vec<u8> {
        let sheet = |seed: u8, encoding| {
            let texels = (0..16 * 8).map(|index| (index as u8 / 3 + seed) % 32).collect();
            Image::new(8, 8, encoding, texels).expect("a sheet").encode()
        };
        let sheets = [sheet(1, Encoding::Raw), sheet(5, Encoding::Packed)];
        let walk = Animation {
            steps: vec![Step { delay: 4, frame: 0 }, Step { delay: 6, frame: 1 }, Step { delay: JUMP, frame: 0 }],
            frames: vec![
                Frame { cells: vec![Cell { size: 5, x: -8, y: -8, u: 0, v: 0 }] },
                Frame {
                    cells: vec![
                        Cell { size: 0, x: -8, y: -8, u: 0, v: 0 },
                        Cell { size: 0x80, x: 0, y: -8, u: 8, v: 0 },
                    ],
                },
            ],
        };
        let stand = Animation { steps: vec![Step { delay: 1, frame: 0 }], frames: vec![walk.frames[0].clone()] };

        let mut out = vec![0u8; HEADER_WORDS * WORD];
        let images_at = out.len();
        let mut offset = WORD * sheets.len();
        for sheet in &sheets {
            out.extend((offset as u32).to_le_bytes());
            offset += sheet.len();
        }
        for sheet in &sheets {
            out.extend(sheet);
        }
        let palettes_at = out.len();
        for index in 0..palettes {
            out.extend(((WORD * palettes + PALETTE_BYTES * index) as u32).to_le_bytes());
        }
        for index in 0..palettes * COLOURS {
            out.extend(((index % COLOURS) as u16 * 0x0421 + (index / COLOURS) as u16).to_le_bytes());
        }
        let animations_at = out.len();
        let encoded = [walk.encode(), stand.encode()];
        let mut offset = HALF * encoded.len();
        for animation in &encoded {
            out.extend((offset as u16).to_le_bytes());
            offset += animation.len();
        }
        for animation in &encoded {
            out.extend(animation);
        }
        let records_at = out.len();
        out.extend([0, 0, 0, 0, 0, 1, 1, 1, 0]);
        out.push(0xFF);
        out.resize(out.len() + free, 0);
        for (index, at) in [images_at, palettes_at, animations_at, records_at].into_iter().enumerate() {
            out[WORD * index..WORD * index + WORD].copy_from_slice(&(at as u32).to_le_bytes());
        }
        out
    }

    /// A container of three sections.
    pub(crate) fn container(free: usize) -> Vec<u8> {
        let sections = [section_bytes(1, free), section_bytes(2, free), section_bytes(8, free)];
        let mut out = CONTAINER_HEAD.to_le_bytes().to_vec();
        let mut start = CONTAINER_HEAD as usize;
        for section in &sections[..CONTAINER_SECTIONS - 1] {
            start += section.len();
            out.extend((start as u32).to_le_bytes());
        }
        for section in &sections {
            out.extend(section);
        }
        out
    }

    #[test]
    fn a_container_decodes_and_encodes_to_its_bytes() {
        let payload = container(0x40);
        let sprites = decode(&payload).expect("decodes");
        assert!(sprites.container);
        let counts: Vec<usize> = sprites.sections.iter().map(|section| section.palettes.len()).collect();
        assert_eq!(counts, [1, 2, 8]);
        let section = &sprites.sections[0];
        assert_eq!(section.images.len(), 2);
        assert_eq!(section.images[1].encoding, Encoding::Packed);
        assert_eq!(section.animations[0].loop_to(), Some(0));
        assert_eq!(
            section.records,
            [
                Record { image: 0, animation: 0, mirror: 0 },
                Record { image: 0, animation: 0, mirror: 1 },
                Record { image: 1, animation: 1, mirror: 0 },
            ]
        );
        assert_eq!(section.image_of(1), Some(1));
        assert_eq!(encode(&sprites).expect("encodes"), payload);
    }

    #[test]
    fn a_bare_section_is_one_character() {
        let payload = section_bytes(2, 0);
        let sprites = decode(&payload).expect("decodes");
        assert!(!sprites.container);
        assert_eq!(sprites.sections.len(), 1);
        assert_eq!(encode(&sprites).expect("encodes"), payload);
    }

    #[test]
    fn an_in_place_edit_changes_only_its_bytes() {
        let payload = section_bytes(1, 0);
        let mut sprites = decode(&payload).expect("decodes");
        sprites.sections[0].palettes[0][3] = 0x7FFF;
        sprites.sections[0].animations[0].steps[1].delay = 9;
        sprites.sections[0].animations[0].frames[1].cells[1].x = 5;
        let edited = encode(&sprites).expect("encodes");
        let changed = (0..payload.len()).filter(|&at| payload[at] != edited[at]).count();
        assert_eq!(changed, 2 + 1 + 1);
        let again = decode(&edited).expect("decodes again");
        assert_eq!(again.sections[0].palettes[0][3], 0x7FFF);
        assert_eq!(again.sections[0].animations[0].steps[1].delay, 9);
        assert_eq!(again.sections[0].animations[0].frames[1].cells[1].x, 5);
    }

    #[test]
    fn an_edited_sheet_moves_the_parts_after_it() {
        let payload = section_bytes(1, 0x200);
        let mut sprites = decode(&payload).expect("decodes");
        let mut texels = sprites.sections[0].images[0].texels().to_vec();
        texels.iter_mut().for_each(|texel| *texel = 7);
        sprites.sections[0].images[0].set_texels(texels.clone()).expect("same size");
        let edited = encode(&sprites).expect("fits in the padding");
        assert_eq!(edited.len(), payload.len());
        let again = decode(&edited).expect("decodes again");
        assert_eq!(again.sections[0].images[0].texels(), &texels[..]);
        assert_eq!(again.sections[0].images[1].texels(), sprites.sections[0].images[1].texels());
        assert_eq!(again.sections[0].palettes, sprites.sections[0].palettes);
        assert_eq!(again.sections[0].animations, sprites.sections[0].animations);
        assert_eq!(again.sections[0].records, sprites.sections[0].records);
    }

    #[test]
    fn a_sheet_that_outgrows_the_section_is_refused() {
        let payload = section_bytes(1, 0);
        let mut sprites = decode(&payload).expect("decodes");
        // Noise does not pack, and a texel past five bits rules out the packed form.
        let texels: Vec<u8> =
            (0..16 * 8u32).map(|index| (index.wrapping_mul(2_654_435_761) >> 24) as u8 | 0x20).collect();
        sprites.sections[0].images[1].set_texels(texels).expect("same size");
        let error = encode(&sprites).expect_err("no room");
        assert!(error.to_string().contains("keeps its"), "{error}");
    }

    #[test]
    fn a_resized_animation_is_refused() {
        let mut sprites = decode(&section_bytes(1, 0x40)).expect("decodes");
        sprites.sections[0].animations[1].steps.push(Step { delay: 1, frame: 0 });
        assert!(encode(&sprites).is_err());
    }

    #[test]
    fn a_header_out_of_order_is_refused() {
        let mut payload = section_bytes(1, 0);
        payload[WORD..2 * WORD].copy_from_slice(&0x08u32.to_le_bytes());
        assert!(decode(&payload).is_err());
        assert!(decode(&[0; 3]).is_err());
    }

    #[test]
    fn describe_names_the_counts_and_the_readers() {
        let sprites = decode(&container(0)).expect("decodes");
        let text = describe(&sprites);
        assert!(text.contains("Section 2: 2 frame sheets, 8 palettes, 2 animations with 3 frames"), "{text}");
        assert!(text.contains("Actor_SelectScriptBank"), "{text}");
    }
}
