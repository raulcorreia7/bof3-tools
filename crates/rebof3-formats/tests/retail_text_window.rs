//! The message window over every retail text table: the font decodes, every message lays out and renders, and
//! the tables rebuild byte for byte.

mod support;

use std::collections::BTreeSet;

use rebof3_emi::Archive;
use rebof3_formats::message::{po, table};
use rebof3_formats::text::font::Font;
use rebof3_formats::text::layout::{self, Context, PageEnd, Shape};
use rebof3_formats::text::render;
use rebof3_formats::{Format, classify};

/// `ETC/FIRST.EMI` entries: the font sheet, the text palette and the frame palette.
const SHEET: usize = 3;
const TEXT_PALETTE: usize = 8;
const FRAME_PALETTE: usize = 9;
/// The largest retail table, in bytes.
const LARGEST: usize = 21547;

fn font(root: &std::path::Path) -> Font {
    let bytes = std::fs::read(root.join("ETC/FIRST.EMI")).expect("FIRST.EMI");
    let archive = Archive::parse(&bytes).expect("archive");
    let payload = |index: usize| archive.payload(&bytes, index).expect("payload").to_vec();
    Font::new(&payload(SHEET), archive.entries()[SHEET].ram_ptr, &payload(TEXT_PALETTE), &payload(FRAME_PALETTE))
        .expect("the font decodes")
}

#[test]
fn every_retail_message_lays_out_renders_and_rebuilds() {
    let Some(root) = support::retail_root() else { return };
    let font = font(&root);
    let (mut tables, mut messages, mut pages, mut proven, mut rebuilt) = (0, 0, 0, 0, 0);
    let (mut choices, mut runtime, mut blank, mut missing_glyphs) = (0, 0, 0, BTreeSet::new());
    let (mut wide, mut largest, mut catalogues) = (0, 0, 0);
    for path in rebof3_emi::archives_under(&root) {
        let bytes = std::fs::read(&path).unwrap();
        let archive = Archive::parse(&bytes).unwrap();
        for index in 0..archive.entries().len() {
            let payload = archive.payload(&bytes, index).unwrap();
            if classify(&archive.entries()[index], payload) != Format::AreaMessageTable {
                continue;
            }
            let Ok(parsed) = table::parse(payload) else { continue };
            if rebof3_formats::text::discover::table_records(payload).is_none() {
                continue;
            }
            tables += 1;
            largest = largest.max(payload.len());
            rebuilt += usize::from(table::build(&parsed).ok().as_deref() == Some(payload));
            let catalogue = po::export(payload).unwrap();
            catalogues += usize::from(po::import(&catalogue, payload).ok().as_deref() == Some(payload));
            for message in &parsed.messages {
                messages += 1;
                let laid = layout::layout(message, Context { table: Some(payload), runtime: None });
                proven += usize::from(laid.unproven.is_empty());
                for page in &laid.pages {
                    pages += 1;
                    choices += usize::from(matches!(page.end, PageEnd::Choice(_)));
                    runtime += page.runtime.len();
                    wide += usize::from(page.line_widths().iter().any(|&w| w > laid.placement.kind.size().0));
                    for glyph in &page.glyphs {
                        if let Shape::Byte(byte) = glyph.shape
                            && !glyph.animated
                            && !font.has_glyph(byte)
                        {
                            missing_glyphs.insert(byte);
                        }
                    }
                    let image = render::window(
                        &font,
                        page,
                        laid.placement.kind.size(),
                        laid.placement.framed,
                        usize::MAX,
                        [0; 3],
                    );
                    blank += usize::from(image.pixels.chunks(4).all(|p| p[..3] == image.pixels[..3]));
                }
            }
        }
    }
    assert_eq!(
        (tables, messages, pages, proven, rebuilt, choices, runtime, blank, wide),
        (165, 10510, 18540, 7933, 165, 721, 3139, 9, 492),
        "tables, messages, pages, messages without runtime text, byte-exact tables, choices, runtime placeholders, blank pages, pages wider than the window"
    );
    assert_eq!(catalogues, 165, "tables whose PO catalogue rebuilds them byte for byte");
    assert!(largest <= table::LIMIT, "the largest retail table, {largest} bytes, is within the limit");
    assert_eq!(largest, LARGEST);
    // 0xFF is the one drawn byte the font sheet has no glyph for.
    assert_eq!(missing_glyphs.into_iter().collect::<Vec<_>>(), [0xFF]);
}

/// Renders one known message to a PNG in the output folder for a visual check.
#[test]
fn a_retail_window_renders_to_png() {
    let Some(root) = support::retail_root() else { return };
    let font = font(&root);
    let bytes = std::fs::read(root.join("WORLD00/AREA030.EMI"));
    let Ok(bytes) = bytes else { return };
    let archive = Archive::parse(&bytes).unwrap();
    let payload = archive.payload(&bytes, 18).unwrap();
    let parsed = table::parse(payload).unwrap();
    let laid = layout::layout(&parsed.messages[1], Context { table: Some(payload), runtime: None });
    let image =
        render::window(&font, &laid.pages[0], laid.placement.kind.size(), laid.placement.framed, usize::MAX, [0; 3]);
    let png = psx_formats::gfx::png::rgba_png(image.width as u32, image.height as u32, &image.pixels);
    support::write(&support::out_dir("retail_text_window"), "window.png", png);
}
