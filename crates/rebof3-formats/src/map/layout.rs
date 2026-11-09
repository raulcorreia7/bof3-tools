//! The map layout as JSON: the byte codes, the objects and the height table of a bundle.

use super::{MapBundle, MapObject};
use crate::error::{Error, Result};

/// The layout of a map bundle as JSON: the byte code of each cell, one row per map row; the object list,
/// each object with its tag and body words; and the height-byte table when the payload ships one. The
/// byte codes and words keep their stored values. Their meanings are not decoded
/// (`wiki: formats/area#0x80104000-map-bundle`, "Map bundle"). The field names follow the C record
/// (`AreaMapBundle` in
/// `decomp/include/world/area.h`).
pub fn export(payload: &[u8]) -> Result<Vec<u8>> {
    let bundle = MapBundle::decode(payload)?;
    let width = usize::from(bundle.width).max(1);
    let cells = usize::from(bundle.width) * usize::from(bundle.height);
    let codes = &bundle.codes[..cells.min(bundle.codes.len())];
    let rows: Vec<&[u8]> = codes.chunks(width).collect();
    let objects: Vec<serde_json::Value> = bundle.objects.iter().map(|object| serde_json::json!({ "tag": object.tag, "words": object.words })).collect();
    let document = serde_json::json!({
        "width": bundle.width,
        "height": bundle.height,
        "byte_codes": rows,
        "objects": objects,
        "heights": bundle.heights,
    });
    Ok(serde_json::to_vec_pretty(&document).expect("a JSON value always serializes"))
}

/// The layout of a map bundle (`<slot>.layout.json`): the byte codes row by row, the objects, and the
/// height table. The field names follow the C record (`AreaMapBundle` in `decomp/include/world/area.h`)
/// and the map bundle format (`wiki: formats/area#0x80104000-map-bundle`).
#[derive(serde::Deserialize)]
struct Layout {
    width: u8,
    height: u8,
    byte_codes: Vec<Vec<u8>>,
    objects: Vec<LayoutObject>,
    heights: Option<Vec<u8>>,
}

/// One object of a layout: its tag and its body words.
#[derive(serde::Deserialize)]
struct LayoutObject {
    tag: u16,
    words: Vec<u32>,
}

/// The stored bytes of a map bundle, from its layout JSON and the stored bundle. The JSON's byte codes, objects and height
/// table replace the stored ones. Every other section keeps its stored value, because the layout does not name it.
pub fn import(json: &[u8], payload: &[u8]) -> Result<Vec<u8>> {
    let layout: Layout = serde_json::from_slice(json).map_err(|error| Error::Import(format!("layout JSON: {error}")))?;
    let mut bundle = MapBundle::decode(payload)?;
    let (width, height) = (usize::from(bundle.width), usize::from(bundle.height));
    if layout.width != bundle.width || layout.height != bundle.height {
        return Err(Error::Import(format!("the layout is {}x{}; the bundle is {width}x{height}", layout.width, layout.height)));
    }
    if layout.byte_codes.len() != height || layout.byte_codes.iter().any(|row| row.len() != width) {
        return Err(Error::Import(format!("the byte codes are not {height} rows of {width}")));
    }
    for (cell, code) in layout.byte_codes.iter().flatten().enumerate() {
        bundle.codes[cell] = *code;
    }
    if layout.heights.as_ref().is_some_and(|table| table.len() != width * height) {
        return Err(Error::Import(format!("the height table is not {} bytes", width * height)));
    }
    bundle.objects = layout.objects.into_iter().map(|object| MapObject { tag: object.tag, words: object.words }).collect();
    bundle.heights = layout.heights;
    Ok(bundle.encode())
}
