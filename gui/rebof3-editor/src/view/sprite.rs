//! A character sprite bank: its sections, each section's node table, and the palettes
//! `GFX_StagePalette` stages from it (`wiki: formats/character`).
//!
//! The node pixel data is not decoded, so the view lists the nodes and edits only the palettes. An
//! LZ77 bank is decoded, changed and encoded again; the encoder's stream differs from the retail one
//! but decodes to the same bank, and an unchanged bank is never encoded.

use std::ops::Range;

use gpui::{AnyElement, Context, Div, div, prelude::*, px};
use rebof3_assets::decode::color_rgba;
use rebof3_emi::character_bank::{self, Bank, COLOURS};
use rebof3_emi::lz77;
use rebof3_formats::Format;

use super::Entry;
use super::palette::{SWATCH, SWATCH_GAP, Sliders, picker};
use crate::app::Editor;
use crate::theme::{MUTED, color};
use crate::widgets::{GAP, GAP_SECTION, GAP_SMALL, mono, muted, outline, separator, strong, swatch, tip};

/// Nodes in a section's table, and the bytes of the count before the offsets.
const NODES: usize = 16;
const WORD: usize = 4;
/// The halfword that opens an image-shaped node's record, at node `+0x10`.
const IMAGE_TAG: u16 = 0x0080;
const TAG_AT: usize = 0x10;
/// Width of a palette label and of a node table column, in pixels.
const LABEL_WIDTH: f32 = 90.0;
const NODE_COLUMN: f32 = 90.0;

#[derive(Default)]
pub struct State {
    /// The picked colour, an index into [`colour_offsets`].
    selected: usize,
    sliders: Sliders,
}

/// The bank bytes: an LZ77 blob decoded, a raw blob as stored.
pub fn decoded(format: Format, payload: &[u8]) -> Result<Vec<u8>, String> {
    match format {
        Format::Lz77Blob => lz77::decode(payload).map_err(|error| error.to_string()),
        _ => Ok(payload.to_vec()),
    }
}

/// The bank offset of every colour, in section, palette and colour order.
pub fn colour_offsets(bank: &Bank) -> Vec<usize> {
    let mut offsets = Vec::new();
    for section in &bank.sections {
        for palette in 0..section.palettes.len() {
            let at = section.palette_at(palette);
            offsets.extend((0..COLOURS).map(|colour| at + 2 * colour));
        }
    }
    offsets
}

/// One entry of a section's node table: its section-relative offset, and the image record's width,
/// count and size when the node opens with the image tag.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Node {
    pub offset: u32,
    pub image: Option<(u16, u32, u32)>,
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + WORD)?.try_into().ok()?))
}

/// The node table of the section that spans `section` in the bank.
pub fn nodes(bank: &[u8], section: Range<usize>) -> Vec<Node> {
    let bytes = &bank[section];
    (0..NODES)
        .filter_map(|index| u32_at(bytes, WORD + WORD * index))
        .map(|offset| {
            let node = usize::try_from(offset).unwrap_or(usize::MAX);
            let image = node
                .checked_add(TAG_AT)
                .filter(|_| u16_at(bytes, node.saturating_add(TAG_AT)) == Some(IMAGE_TAG))
                .and_then(|at| Some((u16_at(bytes, at + 2)?, u32_at(bytes, at + 4)?, u32_at(bytes, at + 8)?)));
            Node { offset, image }
        })
        .collect()
}

/// The payload with the colour at bank offset `at` set to `word`.
pub fn with_colour(format: Format, payload: &[u8], at: usize, word: u16) -> Result<Vec<u8>, String> {
    let mut bank = decoded(format, payload)?;
    bank.get_mut(at..at + 2).ok_or("the colour is past the end of the bank")?.copy_from_slice(&word.to_le_bytes());
    Ok(match format {
        Format::Lz77Blob => lz77::encode(&bank),
        _ => bank,
    })
}

pub fn show(editor: &Editor, entry: &Entry<'_>, cx: &mut Context<Editor>) -> AnyElement {
    let decoded = match decoded(entry.format(), entry.payload()) {
        Ok(decoded) => decoded,
        Err(error) => return muted(format!("The LZ77 stream does not decode: {error}")).into_any_element(),
    };
    let bank = match character_bank::parse(&decoded) {
        Ok(bank) => bank,
        Err(error) => return muted(format!("No palette layer: {error}")).into_any_element(),
    };
    let original = decoded_original(entry);
    let offsets = colour_offsets(&bank);
    let selected = editor.views.sprite.selected.min(offsets.len().saturating_sub(1));

    let mut list = div().id("sprite-bank").flex().flex_col().flex_1().min_h_0().gap(px(GAP_SMALL)).overflow_y_scroll();
    let mut colour = 0;
    for (number, section) in bank.sections.iter().enumerate() {
        let end = bank.sections.get(number + 1).map_or(decoded.len(), |next| next.start);
        list = list
            .child(
                strong(format!("Section {number} at {:#X}, {} bytes", section.start, end - section.start))
                    .mt(px(GAP_SECTION)),
            )
            .child(node_table(&nodes(&decoded, section.start..end)));

        for (palette, words) in section.palettes.iter().enumerate() {
            let mut line = div().flex().items_center().gap(px(SWATCH_GAP));
            line = line.child(mono(format!("palette {palette}")).w(px(LABEL_WIDTH)).text_color(color(MUTED)));
            for &word in words {
                let index = colour;
                let at = offsets[index];
                let edited =
                    original.as_deref().and_then(|bytes| bytes.get(at..at + 2)) != Some(&word.to_le_bytes()[..]);
                line = line.child(
                    swatch(("colour", index), color_rgba(word), SWATCH, outline(index == selected, edited))
                        .tooltip(tip(format!("{at:#X}: {word:#06X}")))
                        .on_click(cx.listener(move |editor, _, _, cx| {
                            editor.views.sprite.selected = index;
                            cx.notify();
                        })),
                );
                colour += 1;
            }
            list = list.child(line);
        }
    }

    let word = offsets.get(selected).and_then(|&at| u16_at(&decoded, at)).unwrap_or_default();
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .gap(px(GAP))
        .child(muted("The node pixel format is not decoded; the palettes are what GFX_StagePalette copies."))
        .child(picker(word, selected, &editor.views.sprite.sliders, set_colour, cx))
        .child(separator(false))
        .child(list)
        .into_any_element()
}

/// The bank as the disc stores it, decoded.
fn decoded_original(entry: &Entry<'_>) -> Option<Vec<u8>> {
    decoded(entry.format(), entry.original()).ok()
}

fn node_table(nodes: &[Node]) -> Div {
    let cell = |text: String| div().w(px(NODE_COLUMN)).flex_none().child(text);
    let mut table = div().flex().flex_col().font_family(crate::fonts::mono()).text_color(color(MUTED));
    table = table.child(
        div()
            .flex()
            .child(cell("node".into()))
            .child(cell("offset".into()))
            .child(cell("width".into()))
            .child(cell("count".into()))
            .child(cell("size".into())),
    );
    for (index, node) in nodes.iter().enumerate() {
        let (width, count, size) = match node.image {
            Some((width, count, size)) => (width.to_string(), count.to_string(), format!("{size:#X}")),
            None => ("-".into(), "-".into(), "-".into()),
        };
        table = table.child(
            div()
                .flex()
                .child(cell(index.to_string()))
                .child(cell(format!("{:#X}", node.offset)))
                .child(cell(width))
                .child(cell(count))
                .child(cell(size)),
        );
    }
    table
}

/// Rewrites colour `index` of the shown bank.
fn set_colour(editor: &mut Editor, index: usize, change: &dyn Fn(u16) -> u16) {
    let Some(entry) = editor.current() else { return };
    let format = entry.format();
    let Ok(decoded) = decoded(format, entry.payload()) else { return };
    let Ok(bank) = character_bank::parse(&decoded) else { return };
    let Some(&at) = colour_offsets(&bank).get(index) else { return };
    let Some(word) = u16_at(&decoded, at) else { return };

    match with_colour(format, entry.payload(), at, change(word)) {
        Ok(payload) => editor.set_payload(payload),
        Err(error) => editor.status = error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One bare section: a node table whose first node is the palette table, one palette of colours
    /// 0 to 31, then an image node tagged with width 16, count 1 and size 0x1000.
    fn section() -> Vec<u8> {
        let palette_table = WORD + NODES * WORD;
        let image = palette_table + WORD + character_bank::PALETTE_BYTES;
        let mut bytes = (NODES as u32).to_le_bytes().to_vec();
        bytes.extend((palette_table as u32).to_le_bytes());
        bytes.extend((image as u32).to_le_bytes());
        bytes.resize(palette_table, 0);
        bytes.extend((WORD as u32).to_le_bytes());
        bytes.extend((0..COLOURS as u16).flat_map(u16::to_le_bytes));
        bytes.extend([0; TAG_AT]);
        bytes.extend(IMAGE_TAG.to_le_bytes());
        bytes.extend(16u16.to_le_bytes());
        bytes.extend(1u32.to_le_bytes());
        bytes.extend(0x1000u32.to_le_bytes());
        bytes
    }

    #[test]
    fn the_node_table_reads_the_image_tag() {
        let bank = section();
        let nodes = nodes(&bank, 0..bank.len());
        assert_eq!(nodes.len(), NODES);
        assert_eq!(nodes[0].image, None, "the palette table is not an image node");
        assert_eq!(nodes[1].image, Some((16, 1, 0x1000)));
        assert_eq!(nodes[2], Node { offset: 0, image: None });
    }

    #[test]
    fn every_colour_has_its_bank_offset() {
        let bank = section();
        let offsets = colour_offsets(&character_bank::parse(&bank).unwrap());
        assert_eq!(offsets.len(), COLOURS);
        assert_eq!(u16_at(&bank, offsets[5]), Some(5));
    }

    #[test]
    fn a_colour_edit_survives_the_lz77_round_trip() {
        let bank = section();
        let at = colour_offsets(&character_bank::parse(&bank).unwrap())[3];

        let raw = with_colour(Format::CharacterBlob, &bank, at, 0x7FFF).unwrap();
        assert_eq!(u16_at(&raw, at), Some(0x7FFF));
        assert_eq!(raw.len(), bank.len());

        let packed = with_colour(Format::Lz77Blob, &lz77::encode(&bank), at, 0x7FFF).unwrap();
        assert_eq!(lz77::decode(&packed).unwrap(), raw);
    }

    /// Every retail sprite bank with a palette layer takes a colour edit and keeps every other byte of
    /// the decoded bank. Point `BOF3_DISC_DIR` at an extracted disc; without it the test skips.
    #[test]
    fn every_retail_bank_takes_a_colour_edit() {
        let Some(disc) = crate::retail_support::retail_disc() else { return };
        let folder = std::env::temp_dir().join(format!("bof3-edit-sprite-{}", std::process::id()));
        let mut project = crate::project::Project::open(disc.as_ref(), &folder).expect("project opens");

        let (mut banks, mut failures) = (0, Vec::new());
        for path in project.archives.clone() {
            let archive = project.load(&path).expect("archive loads");
            for index in 0..archive.entries().len() {
                let format = archive.formats[index];
                if !matches!(format, Format::Lz77Blob | Format::CharacterBlob) {
                    continue;
                }
                let payload = archive.original(index);
                let Ok(bank) = decoded(format, payload) else { continue };
                let Ok(parsed) = character_bank::parse(&bank) else { continue };
                banks += 1;

                let at = colour_offsets(&parsed)[0];
                let word = u16_at(&bank, at).unwrap() ^ 1;
                let edited = with_colour(format, payload, at, word).and_then(|bytes| decoded(format, &bytes));
                let mut expected = bank.clone();
                expected[at..at + 2].copy_from_slice(&word.to_le_bytes());
                if edited.as_ref() != Ok(&expected) {
                    failures.push(format!("{}#{index}", path.0));
                }
            }
        }

        let _ = std::fs::remove_dir_all(&folder);
        eprintln!("sprite banks: {} of {banks} fail a colour edit", failures.len());
        assert!(banks > 0 && failures.is_empty(), "{:?}", failures.iter().take(8).collect::<Vec<_>>());
    }
}
