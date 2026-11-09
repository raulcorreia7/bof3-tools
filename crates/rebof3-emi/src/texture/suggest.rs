//! Ranked depth and CLUT suggestions for a texture no rule joins.
//!
//! The suggestion is computed from the texture's words and the scene's palette rows; it is shown
//! as a suggestion and never applied as a join.
//!
//! - **Depth.** Each depth reads the words as a texel image. Drawn images repeat a texel next to
//!   itself far more often than chance; the wrong depth splits or merges texels and loses that.
//!   The score is the agreement of each texel with its right and lower neighbours, corrected
//!   for the agreement the image's own histogram gives by chance (Cohen's kappa).
//! - **CLUT.** A candidate is a row of the palette band that one entry wrote: column 0 of each
//!   row at 8 bits, each 16-word column at 4 bits. Neighbouring texels with different indices
//!   should get close colours, so the score is one minus their mean colour distance, less the
//!   share of texels that land on a transparent `0x0000` word, plus a bonus when the texture's
//!   own archive wrote the row.

use psx_formats::gfx::texture::{Clut, Depth, texel_indices};

use super::{ARENA_ROW_WORDS, ARENA_VRAM_ROW};
use crate::replay::{ARENA_ROWS, Vram, Writer};

/// Bonus for a row the texture's own archive wrote.
const OWN_ARCHIVE_BONUS: f64 = 0.25;
/// Weight of the transparent share.
const TRANSPARENT_WEIGHT: f64 = 0.5;
/// The brightest 5-bit channel.
const CHANNEL_MAX: f64 = 31.0;

/// A depth and how well the words read at it; `None` is 16 bits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DepthScore {
    /// The depth, or `None` for direct 16-bit words.
    pub depth: Option<Depth>,
    /// Chance-corrected neighbour agreement, at most 1.
    pub score: f64,
}

/// A CLUT row a texture could be drawn with.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    /// The depth the row was scored at.
    pub depth: Depth,
    /// The row's VRAM position.
    pub clut: Clut,
    /// The entry that wrote the row.
    pub palette: Writer,
    /// Higher is better.
    pub score: f64,
}

/// The ranked depths, and the ranked rows at the best indexed depth.
#[derive(Clone, Debug, PartialEq)]
pub struct Suggestion {
    /// Every depth, best first.
    pub depths: Vec<DepthScore>,
    /// Rows at the best 4- or 8-bit depth, best first.
    pub cluts: Vec<Candidate>,
}

impl Suggestion {
    /// The best indexed depth and row, if any row is a candidate.
    pub fn best(&self) -> Option<&Candidate> {
        self.cluts.first()
    }
}

/// Suggests a depth and a row for the `width`×`height` words of a texture, from the palette rows
/// of `vram`. `archive` is the texture's own archive.
pub fn suggest(vram: &Vram, words: &[u16], width: usize, height: usize, archive: &str) -> Suggestion {
    let depths = depth_scores(words, width, height);
    let indexed = depths.iter().find_map(|score| score.depth).unwrap_or(Depth::Four);
    let cluts = clut_candidates(vram, words, width, height, indexed, archive);
    Suggestion { depths, cluts }
}

/// The three depths ranked by chance-corrected neighbour agreement, best first.
pub fn depth_scores(words: &[u16], width: usize, height: usize) -> Vec<DepthScore> {
    let mut scores: Vec<DepthScore> = [Some(Depth::Four), Some(Depth::Eight), None]
        .into_iter()
        .map(|depth| {
            let (texels, row) = match depth {
                Some(depth) => (texels(words, width, height, depth), width * depth.texels_per_word()),
                None => (words[..(width * height).min(words.len())].to_vec(), width),
            };
            DepthScore { depth, score: agreement(&texels, row) }
        })
        .collect();
    scores.sort_by(|left, right| right.score.total_cmp(&left.score));
    scores
}

/// The rows of the palette band one entry wrote, scored for the texture at `depth`, best first.
pub fn clut_candidates(vram: &Vram, words: &[u16], width: usize, height: usize, depth: Depth, archive: &str) -> Vec<Candidate> {
    let colors = depth.colors();
    let row = width * depth.texels_per_word();
    let indices = texels(words, width, height, depth);
    let mut histogram = vec![0usize; colors];
    let mut pairs = vec![0usize; colors * colors];
    for (at, &index) in indices.iter().enumerate() {
        let index = usize::from(index);
        histogram[index] += 1;
        for next in [at + 1, at + row] {
            if (next != at + 1 || next % row != 0)
                && let Some(&other) = indices.get(next)
                && usize::from(other) != index
            {
                pairs[index * colors + usize::from(other)] += 1;
            }
        }
    }
    let texel_count = indices.len().max(1) as f64;
    let pair_count: usize = pairs.iter().sum();

    let columns: Vec<u16> = match depth {
        Depth::Four => (0..ARENA_ROW_WORDS).step_by(16).collect(),
        Depth::Eight => vec![0],
    };
    let mut candidates = Vec::new();
    for y in ARENA_VRAM_ROW..ARENA_VRAM_ROW + ARENA_ROWS as u16 {
        for &x in &columns {
            let Some(clut) = Clut::new(x, y) else { continue };
            let Some(palette) = vram.clut_writer(clut, depth) else { continue };
            let row_words = vram.clut_words(clut, depth);
            let rgb: Vec<[f64; 3]> = row_words.iter().map(|&word| channels(word)).collect();
            let distance: f64 = if pair_count == 0 {
                0.0
            } else {
                pairs.iter().enumerate().filter(|(_, count)| **count != 0).map(|(at, &count)| count as f64 * gap(rgb[at / colors], rgb[at % colors])).sum::<f64>() / pair_count as f64
            };
            let transparent: usize = histogram.iter().zip(&row_words).filter(|(_, word)| **word == 0).map(|(count, _)| count).sum();
            let own = palette.as_entry().is_some_and(|(owner, _)| owner == archive);
            let score = (1.0 - distance) - TRANSPARENT_WEIGHT * transparent as f64 / texel_count + if own { OWN_ARCHIVE_BONUS } else { 0.0 };
            candidates.push(Candidate { depth, clut, palette: palette.clone(), score });
        }
    }
    candidates.sort_by(|left, right| right.score.total_cmp(&left.score));
    candidates
}

/// The texel indices of a `width`×`height` word grid at `depth`, row-major.
fn texels(words: &[u16], width: usize, height: usize, depth: Depth) -> Vec<u16> {
    words.chunks(width.max(1)).take(height).flat_map(|line| texel_indices(line, depth)).map(u16::from).collect()
}

/// Cohen's kappa of each value against its right and lower neighbour; 0 for a flat image.
fn agreement(values: &[u16], row: usize) -> f64 {
    if row == 0 || values.len() < 2 {
        return 0.0;
    }
    let mut equal = 0usize;
    let mut total = 0usize;
    for (at, value) in values.iter().enumerate() {
        if (at + 1) % row != 0
            && let Some(next) = values.get(at + 1)
        {
            total += 1;
            equal += usize::from(next == value);
        }
        if let Some(below) = values.get(at + row) {
            total += 1;
            equal += usize::from(below == value);
        }
    }
    let mut counts = std::collections::HashMap::new();
    for value in values {
        *counts.entry(value).or_insert(0usize) += 1;
    }
    let n = values.len() as f64;
    let chance: f64 = counts.values().map(|&count| (count as f64 / n).powi(2)).sum();
    if total == 0 || chance >= 1.0 {
        return 0.0;
    }
    (equal as f64 / total as f64 - chance) / (1.0 - chance)
}

/// A colour word's 5-bit channels.
fn channels(word: u16) -> [f64; 3] {
    [f64::from(word & 31), f64::from((word >> 5) & 31), f64::from((word >> 10) & 31)]
}

/// The distance of two colours, 0 to 1.
fn gap(left: [f64; 3], right: [f64; 3]) -> f64 {
    let sum: f64 = left.iter().zip(right).map(|(a, b)| (a - b).powi(2)).sum();
    sum.sqrt() / (CHANNEL_MAX * 3f64.sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Entry;
    use psx_formats::gfx::texture::texel_words;

    /// A 64×32-texel image of horizontal bands, four texels per index run.
    fn banded(depth: Depth) -> Vec<u16> {
        let width = 64;
        let indices: Vec<u8> = (0..32 * width).map(|at| ((at % width) / 4 % depth.colors().min(16)) as u8).collect();
        texel_words(&indices, depth)
    }

    #[test]
    fn a_banded_4bit_image_scores_best_at_4_bits() {
        let words = banded(Depth::Four);
        let scores = depth_scores(&words, 16, 32);
        assert_eq!(scores[0].depth, Some(Depth::Four), "{scores:?}");
    }

    #[test]
    fn a_banded_8bit_image_scores_best_at_8_bits() {
        let words = banded(Depth::Eight);
        let scores = depth_scores(&words, 32, 32);
        assert_eq!(scores[0].depth, Some(Depth::Eight), "{scores:?}");
    }

    #[test]
    fn a_flat_image_scores_zero() {
        assert_eq!(agreement(&[5; 64], 8), 0.0);
    }

    #[test]
    fn the_smooth_row_ranks_above_the_noisy_one() {
        // Row 480 holds a grey ramp; row 481 alternates black and white.
        let ramp: Vec<u8> = (0..16u16).flat_map(|level| (0x8000 | (level * 2 * 0x421)).to_le_bytes()).collect();
        let noise: Vec<u8> = (0..16u16).flat_map(|level| (0x8000 | ((level % 2) * 0x7FFF)).to_le_bytes()).collect();
        let entries = [Entry { size: 32, offset: 0, ram_ptr: 0x8003_3800, first4: 0, file_type: 0 }, Entry { size: 32, offset: 0, ram_ptr: 0x8003_3A00, first4: 0, file_type: 0 }];
        let mut vram = Vram::new();
        vram.load("BIN/OTHER.EMI", &entries, &[&ramp, &noise]);
        vram.reset_palette();
        vram.upload_palette();

        let words = banded(Depth::Four);
        let ranked = clut_candidates(&vram, &words, 16, 32, Depth::Four, "BIN/OWN.EMI");
        assert_eq!(ranked.len(), 2, "only fully written rows are candidates");
        assert_eq!((ranked[0].clut.x, ranked[0].clut.y), (0, 480));
    }
}
