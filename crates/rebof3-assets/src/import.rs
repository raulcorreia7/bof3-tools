//! The import direction: the open formats back to the game's own bytes.
//!
//! Each import reads what its export wrote and returns the stored bytes of the
//! entry. A texture PNG is either the 16 bpp view (`<slot>.png`) or an indexed
//! row (`<slot>.clut-<x>-<y>.png`), and [`texture`] tells them apart by the PNG's
//! color type. The owner's disc round-trips byte for byte (`tests/round_trip.rs`).

use psx_formats::adpcm;
use psx_formats::gfx::texture::{self, Depth};
use psx_formats::gfx::{clut, png, vram};
use rebof3_emi::area::map::{MapBundle, MapObject};
use rebof3_emi::encounter::{self, EncounterBlock};
use rebof3_emi::game_tables::{self, PAYLOAD_START, TABLES, Table};

use crate::decode::VAG_RATE;
use crate::error::{Error, Result};
use crate::gltf;
use crate::terrain::{CORNER_STEPS, HEIGHT_SCALE};

/// One fixed table of `ETC/GAME.EMI` entry 0, from its JSON records (`<name>.json`, an array of the records).
/// `base` holds the table's stored bytes. Each named field is written over its base record, so the bytes no
/// field names keep their stored values, and a name keeps its stored bytes while its text is unchanged.
pub fn game_table(table: &str, json: &[u8], base: &[u8]) -> Result<Vec<u8>> {
    let descriptor = *TABLES
        .iter()
        .find(|descriptor| descriptor.name == table)
        .ok_or_else(|| Error::Import(format!("there is no table named {table:?}")))?;
    if base.len() != descriptor.count * descriptor.record {
        return Err(Error::Import(format!(
            "{table} is {} records of {} bytes; its stored bytes are {}",
            descriptor.count,
            descriptor.record,
            base.len()
        )));
    }
    match table {
        "items" => rebuild(descriptor, json, base, game_tables::put_item),
        "vital-items" => rebuild(descriptor, json, base, game_tables::put_vital_item),
        "weapons" => rebuild(descriptor, json, base, game_tables::put_weapon),
        "armor" => rebuild(descriptor, json, base, game_tables::put_armor),
        "accessories" => rebuild(descriptor, json, base, game_tables::put_accessory),
        "shops" => rebuild(descriptor, json, base, game_tables::put_shop),
        "abilities" => rebuild(descriptor, json, base, game_tables::put_ability),
        "level-growth" => rebuild(descriptor, json, base, game_tables::put_growth),
        _ => Err(Error::Import(format!("{table} has no writer"))),
    }
}

/// The characters of `ETC/START.EMI` entry 8 from the JSON of `tables::characters`, written over the stored payload.
pub fn characters(json: &[u8], payload: &[u8]) -> Result<Vec<u8>> {
    Ok(rebof3_emi::characters::import(json, payload)?)
}

/// The palettes of one sprite bank from the JSON of `sprites::character_bank`, written over the decoded bank.
pub fn character_bank(json: &[u8], payload: &[u8]) -> Result<Vec<u8>> {
    Ok(rebof3_emi::character_bank::import(json, payload)?)
}

/// One fixed table of `ETC/GAME.EMI` entry 0, from its JSON records and the stored payload of that entry.
pub fn game_table_in_payload(table: &str, json: &[u8], payload: &[u8]) -> Result<Vec<u8>> {
    let descriptor = TABLES
        .iter()
        .find(|descriptor| descriptor.name == table)
        .ok_or_else(|| Error::Import(format!("there is no table named {table:?}")))?;
    let start = descriptor.archive_offset - PAYLOAD_START;
    let end = start + descriptor.count * descriptor.record;
    let base = payload.get(start..end).ok_or_else(|| Error::Import(format!("the payload ends before {table}")))?;
    game_table(table, json, base)
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
pub fn map_layout(json: &[u8], payload: &[u8]) -> Result<Vec<u8>> {
    let layout: Layout =
        serde_json::from_slice(json).map_err(|error| Error::Import(format!("layout JSON: {error}")))?;
    let mut bundle = MapBundle::decode(payload)?;
    let (width, height) = (usize::from(bundle.width), usize::from(bundle.height));
    if layout.width != bundle.width || layout.height != bundle.height {
        return Err(Error::Import(format!(
            "the layout is {}x{}; the bundle is {width}x{height}",
            layout.width, layout.height
        )));
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
    bundle.objects =
        layout.objects.into_iter().map(|object| MapObject { tag: object.tag, words: object.words }).collect();
    bundle.heights = layout.heights;
    Ok(bundle.encode())
}

/// The encounter block of an area (`area-encounters`), from its JSON and its stored bytes. `base` is the stored
/// block: the bytes no field names keep their stored values.
pub fn encounters(json: &[u8], base: &[u8]) -> Result<Vec<u8>> {
    let block: EncounterBlock =
        serde_json::from_slice(json).map_err(|error| Error::Import(format!("encounter JSON: {error}")))?;
    if base.len() != encounter::BLOCK_BYTES
        || block.encounters.len() != encounter::ENCOUNTERS
        || block.enemies.len() != encounter::ENEMIES
    {
        return Err(Error::Import(format!(
            "an encounter block is {} bytes, {} encounters and {} enemies",
            encounter::BLOCK_BYTES,
            encounter::ENCOUNTERS,
            encounter::ENEMIES
        )));
    }
    let mut bytes = base.to_vec();
    for (index, value) in block.encounters.iter().enumerate() {
        let at = index * encounter::ENCOUNTER_BYTES;
        encounter::put_encounter(&mut bytes[at..at + encounter::ENCOUNTER_BYTES], value).map_err(Error::Import)?;
    }
    for (index, value) in block.enemies.iter().enumerate() {
        let at = encounter::ENEMY_BASE + index * encounter::ENEMY_BYTES;
        encounter::put_enemy(&mut bytes[at..at + encounter::ENEMY_BYTES], value).map_err(Error::Import)?;
    }
    Ok(bytes)
}

/// Parses a table's JSON records and writes each one over its base record.
fn rebuild<T: serde::de::DeserializeOwned>(
    table: Table,
    json: &[u8],
    base: &[u8],
    put: fn(&mut [u8], &T) -> std::result::Result<(), String>,
) -> Result<Vec<u8>> {
    let records: Vec<T> =
        serde_json::from_slice(json).map_err(|error| Error::Import(format!("{} JSON: {error}", table.name)))?;
    if records.len() != table.count {
        return Err(Error::Import(format!(
            "{} has {} records; the table has {}",
            table.name,
            records.len(),
            table.count
        )));
    }
    let mut bytes = base.to_vec();
    for (record, value) in bytes.chunks_exact_mut(table.record).zip(&records) {
        put(record, value).map_err(|reason| Error::Import(format!("{} record: {reason}", table.name)))?;
    }
    Ok(bytes)
}

/// A sequence package (`.seq`) from Standard MIDI Files, one sequence per part and in order. Each part is a MIDI
/// file that [`crate::decode::sequence_midi`] writes, with the sequence id the package gives it.
pub fn sequence(parts: &[(&[u8], u16)]) -> Result<Vec<u8>> {
    let sequences = parts
        .iter()
        .map(|&(midi, seq_id)| psx_formats::sep::Sequence::from_midi(midi, seq_id))
        .collect::<psx_formats::Result<Vec<_>>>()?;
    Ok(psx_formats::sep::Package { sequences }.encode())
}

/// An EMI container with the entries named in `replace` swapped for new payloads. Every other entry keeps its
/// stored payload, type and load address, and the container keeps its version. With no replacement the result is
/// the container's bytes.
pub fn emi(container: &[u8], replace: &[(usize, &[u8])]) -> Result<Vec<u8>> {
    Ok(rebof3_emi::pack::rebuild(container, replace)?)
}

/// Encodes a WAVE file as a VAG: its samples as SPU ADPCM blocks. The file must be mono 16-bit PCM at
/// [`VAG_RATE`], which is what [`crate::decode::pcm_wav`] writes. ADPCM cannot reproduce every sample, so the
/// first encode is lossy for those; a second encode of its decoded samples is exact.
pub fn vag(wav: &[u8]) -> Result<Vec<u8>> {
    Ok(adpcm::encode_stream(&read_pcm_wav(wav)?))
}

/// The samples of a mono 16-bit PCM WAVE file at [`VAG_RATE`].
fn read_pcm_wav(bytes: &[u8]) -> Result<Vec<i16>> {
    let bad = |reason: &str| Error::Import(format!("the WAVE file {reason}"));
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(bad("is not a RIFF WAVE file"));
    }
    let (mut format, mut data) = (None, None);
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let size = u32::from_le_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]]) as usize;
        let body = bytes.get(at + 8..at + 8 + size).ok_or_else(|| bad("has a chunk past its end"))?;
        match &bytes[at..at + 4] {
            b"fmt " => format = Some(body),
            b"data" => data = Some(body),
            _ => {}
        }
        at += 8 + size + (size & 1);
    }
    let format = format.filter(|format| format.len() >= 16).ok_or_else(|| bad("has no format chunk"))?;
    let data = data.ok_or_else(|| bad("has no data chunk"))?;
    let tag = u16::from_le_bytes([format[0], format[1]]);
    let channels = u16::from_le_bytes([format[2], format[3]]);
    let rate = u32::from_le_bytes([format[4], format[5], format[6], format[7]]);
    let bits = u16::from_le_bytes([format[14], format[15]]);
    if tag != 1 || channels != 1 || rate != VAG_RATE || bits != 16 {
        return Err(bad("must be mono 16-bit PCM at 44100 Hz"));
    }
    Ok(data.as_chunks::<2>().0.iter().map(|pair| i16::from_le_bytes(*pair)).collect())
}

/// The terrain mesh's corner heights, written back into the map bundle it came from.
///
/// `mesh` is the binary glTF or the OBJ text that the export writes, and `payload` is that bundle's
/// stored bytes. A corner's height is its `y` divided by [`HEIGHT_SCALE`]. The mesh does not carry the
/// tiles, objects or height table, so those sections come from `payload`. A corner that the mesh moves
/// in `x` or `z` is an error.
pub fn terrain(mesh: &[u8], payload: &[u8]) -> Result<Vec<u8>> {
    let mut bundle = MapBundle::decode(payload)?;
    let positions = if mesh.starts_with(b"glTF") {
        gltf::read_positions(mesh).map_err(Error::Import)?
    } else {
        obj_positions(mesh)?
    };
    let corners = 4 * bundle.cells.len();
    if positions.len() != corners {
        return Err(Error::Import(format!("the mesh has {} corners; the bundle needs {corners}", positions.len())));
    }
    let width = usize::from(bundle.width).max(1);
    for (cell, record) in bundle.cells.iter_mut().enumerate() {
        let (column, row) = ((cell % width) as f32, (cell / width) as f32);
        for (corner, [step_x, step_z]) in CORNER_STEPS.iter().enumerate() {
            let [x, y, z] = positions[4 * cell + corner];
            if x != column + *step_x as f32 || z != row + *step_z as f32 {
                return Err(Error::Import(format!("corner {corner} of cell {cell} is not at its cell")));
            }
            record.heights[corner] = height_byte(y)
                .ok_or_else(|| Error::Import(format!("corner {corner} of cell {cell} is at {y}, not a height step")))?;
        }
    }
    Ok(bundle.encode())
}

/// The height byte of a corner at `y`. `y` is the byte times [`HEIGHT_SCALE`], so it must be a whole step
/// that an `i8` holds.
fn height_byte(y: f32) -> Option<i8> {
    let step = y / HEIGHT_SCALE as f32;
    (step.fract() == 0.0 && (-128.0..=127.0).contains(&step)).then_some(step as i8)
}

/// The vertices of an OBJ text: its `v` lines, in file order.
fn obj_positions(mesh: &[u8]) -> Result<Vec<[f32; 3]>> {
    let text = std::str::from_utf8(mesh).map_err(|error| Error::Import(format!("the OBJ is not text: {error}")))?;
    let mut positions = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("v ") else {
            continue;
        };
        let values: std::result::Result<Vec<f32>, _> = rest.split_whitespace().map(str::parse).collect();
        match values.as_deref() {
            Ok([x, y, z]) => positions.push([*x, *y, *z]),
            _ => return Err(Error::Import(format!("the OBJ vertex {rest:?} is not three numbers"))),
        }
    }
    Ok(positions)
}

/// Reads a palette's JSON (`<slot>.json`) back to its stored bytes: the `word` of each color, in order.
/// The `rgba` and `stp` fields are views of the word and are not read.
pub fn palette(json: &[u8]) -> Result<Vec<u8>> {
    let document: serde_json::Value =
        serde_json::from_slice(json).map_err(|error| Error::Import(format!("palette JSON: {error}")))?;
    if document["format"] != "psx-clut" {
        return Err(Error::Import("the JSON is not a psx-clut palette".into()));
    }
    let words = document["colors"]
        .as_array()
        .and_then(|colors| {
            colors.iter().map(|color| color["word"].as_u64().and_then(|word| u16::try_from(word).ok())).collect()
        })
        .ok_or_else(|| Error::Import("each palette color needs a 16-bit word".into()))?;
    Ok(clut::Palette::from_words(words)?.encode())
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
pub fn texture(bytes: &[u8]) -> Result<TextureImport> {
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
