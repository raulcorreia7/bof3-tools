//! What a laid-out message does that the window cannot show as the author meant.
//! pages, `wiki: data/text`). Only text the codec cannot encode and a table past [`crate::message::table::LIMIT`] are

use super::font::{CELL_HEIGHT, Font};
use super::layout::{GLYPH_DROP, LINE_STEP, Layout, Shape, TEXT_ORIGIN};

/// One finding, on a page (counted from 0).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Problem {
    pub page: usize,
    pub text: String,
}

/// The findings for `layout`, in page order.
pub fn problems(layout: &Layout, font: &Font) -> Vec<Problem> {
    let (width, height) = layout.placement.kind.size();
    let mut found = Vec::new();
    for (index, page) in layout.pages.iter().enumerate() {
        let mut say = |text: String| found.push(Problem { page: index, text });
        for (line, right) in page.line_widths().into_iter().enumerate() {
            if right > width {
                say(format!("line {} reaches x {right}, past the {width} pixel window", line + 1));
            }
        }
        let visible = (0..page.lines)
            .take_while(|&line| TEXT_ORIGIN.1 + line as i32 * LINE_STEP + GLYPH_DROP + CELL_HEIGHT as i32 <= height)
            .count();
        if page.lines > visible {
            say(format!("{} lines on the page; the {height} pixel window shows {visible}", page.lines));
        }
        let mut seen = Vec::new();
        for glyph in &page.glyphs {
            if let Shape::Byte(byte) = glyph.shape
                && !glyph.animated
                && !font.has_glyph(byte)
                && !seen.contains(&byte)
            {
                seen.push(byte);
                say(format!("byte {byte:02X} has no picture in the font"));
            }
        }
    }
    for unproven in &layout.unproven {
        found.push(Problem { page: 0, text: format!("offset {}: {}", unproven.offset, unproven.why) });
    }
    found
}
