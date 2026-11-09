//! The files a format's decoder adds to the stored bytes of an asset.

use rebof3_formats::Format;

/// Kept for the sprite view, which the sprite lane edits; new callers use `rebof3_formats::clut::color_rgba`.
pub use rebof3_formats::clut::color_rgba;
pub use rebof3_formats::registry::Derived;

/// The files a format's decoder adds to the stored bytes.
pub fn derive(format: Format, load_arg: u32, payload: &[u8]) -> rebof3_formats::Result<Vec<Derived>> {
    match format.describe().derive {
        Some(derive) => derive(load_arg, payload),
        None => Ok(Vec::new()),
    }
}
