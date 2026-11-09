//! A frame sheet: one image record of a section's image table.
//!
//! `Actor_SelectScriptBank` (`exe/slus_004_22@0x8014DC08`) queues the record `table + table[n]` for
//! upload, and `GFX_UpdateTransferQueue` (`exe/slus_004_22@0x8014E6D0`) reads it as a rectangle and a
//! kind:
//!
//! | Offset | Size | Field | Meaning |
//! | -----: | ---: | ----- | ------- |
//! | `0x00` | 2 | `width` | rectangle width in VRAM words |
//! | `0x02` | 2 | `height` | rectangle height in rows |
//! | `0x04` | 4 | `kind` | 1: `GFX_ExpandRgb555`, 2: `GFX_DecodeLzss`, otherwise a plain `LoadImage` |
//! | `0x08` | - | `body` | the words (kind 0), the packed texels (kind 1), or a byte count and the LZSS stream (kind 2) |
//!
//! The rectangle holds `width * height * 2` bytes: one 8-bit texel per byte, since the field draw sets
//! the 8-bit texture mode (`Area_DrawObjectSprite`, `tpage` depth from work `+0x28`). Kind 1 packs six
//! 5-bit texels in each word, at bits 0, 5, 10, 16, 21 and 26 (`GFX_ExpandRgb555`,
//! `exe/slus_004_22@0x8014E7F8`), so it holds indices into one 32-colour palette.

use rebof3_emi::lzss;

use crate::error::{Error, Result};

/// Bytes before the body: width, height and kind.
pub const HEADER_BYTES: usize = 8;
/// Bytes of the byte count before an LZSS stream.
const LZSS_COUNT_BYTES: usize = 4;
/// Texels in one VRAM word at 8 bits per texel.
pub const TEXELS_PER_WORD: usize = 2;
/// Texels one packed word holds, and the bit each starts at.
const PACKED_TEXELS: usize = 6;
const PACKED_SHIFTS: [u32; PACKED_TEXELS] = [0, 5, 10, 16, 21, 26];
/// The bits of one packed texel.
const PACKED_MASK: u32 = 0x1F;
/// The largest index a packed texel holds.
pub const PACKED_MAX: u8 = PACKED_MASK as u8;
/// Records are laid out on word boundaries.
const ALIGN: usize = 4;

/// How a record stores its texels: the `kind` word `GFX_UpdateTransferQueue` switches on.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Encoding {
    /// The texel bytes as `LoadImage` takes them.
    Raw,
    /// Six 5-bit texels per word (`GFX_ExpandRgb555`, kind 1).
    Packed,
    /// An LZSS stream after the byte count (`GFX_DecodeLzss`, kind 2).
    Lzss,
}

impl Encoding {
    /// The kind word of a record.
    pub fn kind(self) -> u32 {
        match self {
            Self::Raw => 0,
            Self::Packed => 1,
            Self::Lzss => 2,
        }
    }

    /// The encoding of a kind word: `GFX_UpdateTransferQueue` uploads any kind but 1 and 2 as raw words.
    pub fn of_kind(kind: u32) -> Self {
        match kind {
            1 => Self::Packed,
            2 => Self::Lzss,
            _ => Self::Raw,
        }
    }

    /// The name the views and the sheet JSON use.
    pub fn label(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Packed => "packed 5-bit",
            Self::Lzss => "LZSS",
        }
    }
}

/// One frame sheet: its rectangle, how it is stored, and its texels.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Image {
    /// Rectangle width in VRAM words; the sheet is twice as many texels wide.
    pub width: u16,
    /// Rectangle height in rows.
    pub height: u16,
    /// How the record stores the texels.
    pub encoding: Encoding,
    /// The kind word as stored; a raw record keeps any kind but 1 and 2.
    kind: u32,
    /// One 8-bit index per texel, row-major.
    texels: Vec<u8>,
    /// The record's bytes as stored, up to the next record, while the texels are unchanged.
    stored: Option<Vec<u8>>,
}

impl Image {
    /// The record at the start of `span`, which runs to the next record. The span is kept, so an
    /// unchanged image encodes to it again.
    pub fn decode(span: &[u8]) -> Result<Self> {
        let [width, height] = [half(span, 0)?, half(span, 2)?];
        let kind = word(span, 4)?;
        let encoding = Encoding::of_kind(kind);
        let count = usize::from(width) * usize::from(height) * TEXELS_PER_WORD;
        let body = &span[HEADER_BYTES..];

        let texels = match encoding {
            Encoding::Raw => body.get(..count).map(<[u8]>::to_vec).ok_or_else(|| short(count, body.len()))?,
            Encoding::Packed => unpack(body, count)?,
            Encoding::Lzss => {
                let stream = body.get(LZSS_COUNT_BYTES..).ok_or_else(|| short(LZSS_COUNT_BYTES, body.len()))?;
                lzss::decode(stream, count)?.0
            }
        };
        Ok(Self { width, height, encoding, kind, texels, stored: Some(span.to_vec()) })
    }

    /// A new image of `width` words by `height` rows, stored as `encoding`.
    pub fn new(width: u16, height: u16, encoding: Encoding, texels: Vec<u8>) -> Result<Self> {
        let image = Self { width, height, encoding, kind: encoding.kind(), texels: Vec::new(), stored: None };
        if texels.len() != image.texel_count() {
            return Err(Error::SpriteBank(format!(
                "a {}x{} sheet holds {} texels, not {}",
                image.texel_width(),
                height,
                image.texel_count(),
                texels.len()
            )));
        }
        Ok(Self { texels, ..image })
    }

    /// Texels per row.
    pub fn texel_width(&self) -> usize {
        usize::from(self.width) * TEXELS_PER_WORD
    }

    /// Texels in the sheet.
    pub fn texel_count(&self) -> usize {
        self.texel_width() * usize::from(self.height)
    }

    /// The texel indices, row-major.
    pub fn texels(&self) -> &[u8] {
        &self.texels
    }

    /// The texel at column `u`, row `v`, when the sheet has it.
    pub fn texel(&self, u: usize, v: usize) -> Option<u8> {
        (u < self.texel_width()).then(|| self.texels.get(v * self.texel_width() + u).copied()).flatten()
    }

    /// The sheet as VRAM words, row-major: two texels per word, the first in the low byte.
    pub fn words(&self) -> Vec<u16> {
        self.texels.as_chunks::<TEXELS_PER_WORD>().0.iter().map(|pair| u16::from_le_bytes(*pair)).collect()
    }

    /// Replaces the texels. A change drops the stored record, so [`Image::encode`] packs the sheet again.
    pub fn set_texels(&mut self, texels: Vec<u8>) -> Result<()> {
        if texels.len() != self.texel_count() {
            return Err(Error::SpriteBank(format!("a sheet of {} texels takes {}", self.texel_count(), texels.len())));
        }
        if texels != self.texels {
            self.texels = texels;
            self.stored = None;
        }
        Ok(())
    }

    /// Whether the image still holds its stored bytes.
    pub fn is_stored(&self) -> bool {
        self.stored.is_some()
    }

    /// The record bytes: the stored span while the texels are unchanged, otherwise the smallest record the
    /// transfer queue reads back to the texels, padded to a word.
    pub fn encode(&self) -> Vec<u8> {
        if let Some(stored) = &self.stored {
            return stored.clone();
        }
        let mut best: Option<(Encoding, Vec<u8>)> = None;
        for encoding in [Encoding::Lzss, Encoding::Packed, Encoding::Raw] {
            let Some(body) = self.body(encoding) else { continue };
            if best.as_ref().is_none_or(|(_, current)| body.len() < current.len()) {
                best = Some((encoding, body));
            }
        }
        let (encoding, body) = best.expect("a raw body always exists");

        let mut out = Vec::with_capacity(HEADER_BYTES + body.len() + ALIGN);
        out.extend(self.width.to_le_bytes());
        out.extend(self.height.to_le_bytes());
        out.extend(encoding.kind().to_le_bytes());
        out.extend(body);
        out.resize(out.len().next_multiple_of(ALIGN), 0);
        out
    }

    /// The body of a record in `encoding`, or `None` when the texels do not fit it.
    fn body(&self, encoding: Encoding) -> Option<Vec<u8>> {
        match encoding {
            Encoding::Raw => Some(self.texels.clone()),
            Encoding::Packed => pack(&self.texels),
            Encoding::Lzss => {
                let mut body = (self.texels.len() as u32).to_le_bytes().to_vec();
                body.extend(lzss::encode(&self.texels));
                Some(body)
            }
        }
    }

    /// The kind word the record stores.
    pub fn kind(&self) -> u32 {
        self.kind
    }
}

/// `count` texels from packed words.
fn unpack(body: &[u8], count: usize) -> Result<Vec<u8>> {
    let words = count.div_ceil(PACKED_TEXELS);
    let bytes = body.get(..words * 4).ok_or_else(|| short(words * 4, body.len()))?;
    let mut texels = Vec::with_capacity(words * PACKED_TEXELS);
    for chunk in bytes.as_chunks::<4>().0 {
        let packed = u32::from_le_bytes(*chunk);
        texels.extend(PACKED_SHIFTS.iter().map(|shift| ((packed >> shift) & PACKED_MASK) as u8));
    }
    texels.truncate(count);
    Ok(texels)
}

/// Packed words for `texels`, or `None` when one needs more than five bits.
fn pack(texels: &[u8]) -> Option<Vec<u8>> {
    if texels.iter().any(|&texel| texel > PACKED_MAX) {
        return None;
    }
    let mut out = Vec::with_capacity(texels.len().div_ceil(PACKED_TEXELS) * 4);
    for group in texels.chunks(PACKED_TEXELS) {
        let packed =
            group.iter().zip(PACKED_SHIFTS).fold(0u32, |word, (&texel, shift)| word | u32::from(texel) << shift);
        out.extend(packed.to_le_bytes());
    }
    Some(out)
}

fn half(bytes: &[u8], at: usize) -> Result<u16> {
    bytes.get(at..at + 2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).ok_or_else(|| short(at + 2, bytes.len()))
}

fn word(bytes: &[u8], at: usize) -> Result<u32> {
    bytes
        .get(at..at + 4)
        .map(|quad| u32::from_le_bytes([quad[0], quad[1], quad[2], quad[3]]))
        .ok_or_else(|| short(at + 4, bytes.len()))
}

fn short(needed: usize, have: usize) -> Error {
    Error::SpriteBank(format!("a frame sheet needs {needed} bytes; its record has {have}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Texels below 32 in a pattern with runs, so every encoding applies.
    fn texels(width: u16, height: u16) -> Vec<u8> {
        (0..usize::from(width) * usize::from(height) * TEXELS_PER_WORD).map(|index| ((index / 5) % 32) as u8).collect()
    }

    #[test]
    fn every_encoding_reads_back_to_its_texels() {
        let texels = texels(8, 4);
        for encoding in [Encoding::Raw, Encoding::Packed, Encoding::Lzss] {
            let image = Image::new(8, 4, encoding, texels.clone()).expect("a sheet");
            let mut record = image.encode();
            // Force the encoding under test by rewriting the record from its body.
            record.truncate(HEADER_BYTES);
            record[4..8].copy_from_slice(&encoding.kind().to_le_bytes());
            record.extend(image.body(encoding).expect("the texels fit"));
            let decoded = Image::decode(&record).expect("decodes");
            assert_eq!(decoded.texels(), &texels[..], "{encoding:?}");
            assert_eq!(decoded.encoding, encoding);
        }
    }

    #[test]
    fn a_stored_record_encodes_to_its_own_bytes() {
        let image = Image::new(8, 4, Encoding::Raw, texels(8, 4)).expect("a sheet");
        let mut span = image.encode();
        span.extend([0xAB, 0xCD, 0, 0]);
        let decoded = Image::decode(&span).expect("decodes");
        assert_eq!(decoded.encode(), span, "the gap after the record is kept");
    }

    #[test]
    fn an_edit_packs_the_sheet_again() {
        let image = Image::new(8, 4, Encoding::Raw, texels(8, 4)).expect("a sheet");
        let mut decoded = Image::decode(&image.encode()).expect("decodes");
        let mut changed = decoded.texels().to_vec();
        changed[3] = 31;
        decoded.set_texels(changed.clone()).expect("same size");
        assert!(!decoded.is_stored());
        let again = Image::decode(&decoded.encode()).expect("the new record decodes");
        assert_eq!(again.texels(), &changed[..]);
        assert!(again.encoding != Encoding::Raw, "a smaller encoding than raw exists");
    }

    #[test]
    fn a_texel_past_five_bits_is_never_packed() {
        let mut texels = texels(8, 4);
        texels[0] = 40;
        let image = Image::new(8, 4, Encoding::Packed, texels).expect("a sheet");
        assert_eq!(image.body(Encoding::Packed), None);
        let decoded = Image::decode(&image.encode()).expect("decodes");
        assert_ne!(decoded.encoding, Encoding::Packed);
        assert_eq!(decoded.texels()[0], 40);
    }

    #[test]
    fn packed_texels_skip_bit_15() {
        let word = 1 | 2 << 5 | 3 << 10 | 1 << 15 | 4 << 16 | 5 << 21 | 6 << 26;
        let texels = unpack(&u32::to_le_bytes(word), PACKED_TEXELS).expect("one word");
        assert_eq!(texels, [1, 2, 3, 4, 5, 6]);
    }
}
