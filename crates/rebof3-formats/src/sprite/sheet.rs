//! A section as a PNG sprite sheet and an Aseprite JSON file.
//!
//! The sheet is an 8-bit indexed PNG with one row per animation a record plays and one cell per frame
//! of it, every cell the size of the box around all those frames, so the actor's position is the same
//! point of each cell. A pixel is the sheet texel the frame shows there ([`super::compose`]); where no
//! cell reaches it is index 0. The PNG palette is the section's chosen palette, colours 32 to 255 are
//! a marker for indices no palette colour gives, and the colour `0x0000` is transparent.
//!
//! The JSON is Aseprite's array form: one frame per step that shows a frame, with the step's time as
//! its `duration`, and one `forward` tag per animation. `meta.bof3` holds what Aseprite has no field
//! for: the section and palette, the actor's position in each cell, each pose's cell, the stream
//! position of each step and each animation's loop point.
//!
//! [`import`] reads both back. A pixel goes to the sheet texel its cell showed when it was exported, so
//! a texel several frames share takes the one change made to it, and two different changes are refused.
//! A palette colour, a step's duration and a texel come back only when they changed; everything else
//! stays as stored, so an unchanged sheet imports to the stored section.

use std::collections::{BTreeMap, HashMap};

use psx_formats::gfx::clut::{view_to_word, word_to_view};
use psx_formats::gfx::png::{self, Picture};
use serde::{Deserialize, Serialize};

use super::{Animation, Bounds, COLOURS, CharacterSprites, Placement, Section, SpriteSizes, Stage, animation_bounds, compose};
use crate::animation::{TICK_MICROS, WRAPPED_TICKS, duration_ms};
use crate::error::{Error, Result};
use rebof3_emi::replay::Vram;

const MICROS_PER_MILLI: f64 = 1000.0;
/// The colours of an 8-bit PNG palette.
const PNG_COLOURS: usize = 256;
const PNG_BITS: u8 = 8;
/// The PNG colour of an index no palette colour gives: opaque magenta.
const MARKER: [u8; 4] = [0xFF, 0x00, 0xFF, 0xFF];
/// The index of a pixel no cell reaches.
const BACKGROUND: u8 = 0;
/// The Aseprite fields this writer fills with fixed values.
const APP: &str = "https://github.com/raulcorreia7/bof3-tools";
const FORMAT_VERSION: &str = "1";
const PIXEL_FORMAT: &str = "I8";
const SCALE: &str = "1";
const DIRECTION: &str = "forward";
const TAG_COLOUR: &str = "#000000ff";
const LAYER: &str = "sprite";
const OPAQUE: u8 = 255;
const BLEND: &str = "normal";

/// The delay byte of a duration, when one gives it.
fn delay_of(ms: u32) -> Option<u8> {
    let ticks = (f64::from(ms) * MICROS_PER_MILLI / TICK_MICROS).round() as u32;
    if ticks == WRAPPED_TICKS {
        Some(0)
    } else if (1..u32::from(super::JUMP)).contains(&ticks) {
        Some(ticks as u8)
    } else {
        None
    }
}

/// The two files of an export.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Sheet {
    pub png: Vec<u8>,
    pub json: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct Json {
    frames: Vec<JsonFrame>,
    meta: Meta,
}

#[derive(Serialize, Deserialize)]
struct JsonFrame {
    filename: String,
    frame: Rect,
    rotated: bool,
    trimmed: bool,
    #[serde(rename = "spriteSourceSize")]
    sprite_source_size: Rect,
    #[serde(rename = "sourceSize")]
    source_size: Size,
    duration: u32,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
struct Rect {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
struct Size {
    w: usize,
    h: usize,
}

#[derive(Serialize, Deserialize)]
struct Meta {
    app: String,
    version: String,
    image: String,
    format: String,
    size: Size,
    scale: String,
    #[serde(rename = "frameTags")]
    frame_tags: Vec<Tag>,
    layers: Vec<Layer>,
    slices: Vec<serde_json::Value>,
    bof3: Extension,
}

#[derive(Serialize, Deserialize)]
struct Tag {
    name: String,
    from: usize,
    to: usize,
    direction: String,
    color: String,
}

#[derive(Serialize, Deserialize)]
struct Layer {
    name: String,
    opacity: u8,
    #[serde(rename = "blendMode")]
    blend_mode: String,
}

/// What the sheet means to the game, past Aseprite's fields.
#[derive(Serialize, Deserialize)]
struct Extension {
    section: usize,
    palette: usize,
    /// The actor's position in every cell.
    origin: Point,
    cell: Size,
    animations: Vec<Exported>,
    poses: Vec<Pose>,
    /// Animations no record plays, so no sheet is known for them; they are not in the PNG.
    unplaced: Vec<usize>,
    /// Cell texels past their sheet, which the frame's other VRAM gives at run time.
    outside: usize,
    /// Pixels whose index the palette has no colour for.
    unresolved: usize,
    evidence: String,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
struct Point {
    x: i32,
    y: i32,
}

#[derive(Serialize, Deserialize)]
struct Exported {
    animation: usize,
    image: usize,
    /// The stream position of each Aseprite frame of the animation.
    steps: Vec<usize>,
    loop_to: Option<usize>,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
struct Pose {
    animation: usize,
    frame: usize,
    x: usize,
    y: usize,
}

/// One stage per sheet, built when first asked for.
struct Stages<'a> {
    section: &'a Section,
    palette: usize,
    built: HashMap<usize, Stage>,
}

impl<'a> Stages<'a> {
    fn new(section: &'a Section, palette: usize) -> Result<Self> {
        if palette >= section.palettes.len() {
            return Err(Error::CharacterSprites(format!("palette {palette} of a section with {}", section.palettes.len())));
        }
        Ok(Self { section, palette, built: HashMap::new() })
    }

    fn get(&mut self, image: usize) -> &Stage {
        let (section, palette) = (self.section, self.palette);
        self.built.entry(image).or_insert_with(|| {
            let sheet = (image, &section.images[image]);
            Stage::new(Vram::new(), sheet, (palette, &section.palettes[palette]), Placement::default())
        })
    }
}

/// The animations a record plays, with their sheets, in table order.
fn placed(section: &Section) -> Vec<(usize, usize)> {
    (0..section.animations.len()).filter_map(|animation| Some((animation, section.image_of(animation)?))).collect()
}

/// The PNG and JSON of section `section` of `sprites` drawn with palette `palette`. `image` is the PNG's
/// file name, which the JSON names.
pub fn export(sprites: &CharacterSprites, section: usize, palette: usize, sizes: &SpriteSizes, image: &str) -> Result<Sheet> {
    let part = sprites.sections.get(section).ok_or_else(|| missing("section", section))?;
    export_section(part, section, palette, sizes, image)
}

/// `sprites` with the colours, texels and step durations of a sheet [`export`] wrote, in the section the
/// sheet names.
pub fn import(sprites: &CharacterSprites, sizes: &SpriteSizes, json: &[u8], png: &[u8]) -> Result<CharacterSprites> {
    let section = read_json(json)?.meta.bof3.section;
    let part = sprites.sections.get(section).ok_or_else(|| missing("section", section))?;
    let mut out = sprites.clone();
    out.sections[section] = import_section(part, sizes, json, png)?;
    Ok(out)
}

/// The sheet of one section; `number` is the section's place in its sprites, which the JSON records.
pub fn export_section(section: &Section, number: usize, palette: usize, sizes: &SpriteSizes, image: &str) -> Result<Sheet> {
    let mut stages = Stages::new(section, palette)?;
    let placed = placed(section);
    let bounds = placed
        .iter()
        .filter_map(|&(animation, _)| animation_bounds(&section.animations[animation], sizes, false))
        .reduce(Bounds::union)
        .ok_or_else(|| Error::CharacterSprites("no record plays an animation with a cell".into()))?;
    let (cell_width, cell_height) = (bounds.width(), bounds.height());
    let columns = placed.iter().map(|&(animation, _)| section.animations[animation].frames.len()).max().unwrap_or(0);
    let (width, height) = (columns.max(1) * cell_width, placed.len() * cell_height);

    let mut indices = vec![BACKGROUND; width * height];
    let (mut poses, mut outside, mut unresolved) = (Vec::new(), 0, 0);
    for (row, &(animation, sheet)) in placed.iter().enumerate() {
        for (column, frame) in section.animations[animation].frames.iter().enumerate() {
            let (x, y) = (column * cell_width, row * cell_height);
            let composite = compose(stages.get(sheet), frame, sizes, false, bounds);
            outside += composite.outside;
            unresolved += composite.unresolved_colours();
            for line in 0..cell_height {
                for dot in 0..cell_width {
                    if let Some(pixel) = composite.pixel(dot, line) {
                        indices[(y + line) * width + x + dot] = pixel.texel.index;
                    }
                }
            }
            poses.push(Pose { animation, frame: column, x, y });
        }
    }

    let (mut frames, mut tags, mut exported) = (Vec::new(), Vec::new(), Vec::new());
    for (row, &(animation, sheet)) in placed.iter().enumerate() {
        let shown = shown_steps(&section.animations[animation], animation)?;
        if shown.is_empty() {
            continue;
        }
        let from = frames.len();
        for &(position, frame, delay) in &shown {
            let rect = Rect { x: frame * cell_width, y: row * cell_height, w: cell_width, h: cell_height };
            frames.push(JsonFrame {
                filename: format!("animation {animation} step {position}"),
                frame: rect,
                rotated: false,
                trimmed: false,
                sprite_source_size: Rect { x: 0, y: 0, w: cell_width, h: cell_height },
                source_size: Size { w: cell_width, h: cell_height },
                duration: duration_ms(delay),
            });
        }
        tags.push(Tag { name: format!("animation {animation}"), from, to: frames.len() - 1, direction: DIRECTION.into(), color: TAG_COLOUR.into() });
        exported.push(Exported { animation, image: sheet, steps: shown.iter().map(|&(position, _, _)| position).collect(), loop_to: section.animations[animation].loop_to() });
    }

    let unplaced = (0..section.animations.len()).filter(|&animation| section.image_of(animation).is_none()).collect();
    let json = Json {
        frames,
        meta: Meta {
            app: APP.into(),
            version: FORMAT_VERSION.into(),
            image: image.into(),
            format: PIXEL_FORMAT.into(),
            size: Size { w: width, h: height },
            scale: SCALE.into(),
            frame_tags: tags,
            layers: vec![Layer { name: LAYER.into(), opacity: OPAQUE, blend_mode: BLEND.into() }],
            slices: Vec::new(),
            bof3: Extension {
                section: number,
                palette,
                origin: Point { x: -bounds.left, y: -bounds.top },
                cell: Size { w: cell_width, h: cell_height },
                animations: exported,
                poses,
                unplaced,
                outside,
                unresolved,
                evidence: super::EVIDENCE.into(),
            },
        },
    };

    let png = png::indexed_png(width as u32, height as u32, PNG_BITS, &png_colours(&section.palettes[palette]), &indices);
    let json = serde_json::to_vec_pretty(&json).expect("a sheet serializes");
    Ok(Sheet { png, json })
}

/// `section` with the colours, texels and step durations of a sheet [`export_section`] wrote.
pub fn import_section(section: &Section, sizes: &SpriteSizes, json: &[u8], png: &[u8]) -> Result<Section> {
    let json = read_json(json)?;
    let picture = match png::read(png)? {
        Picture::Indexed(picture) if picture.bits == PNG_BITS => picture,
        _ => return Err(Error::CharacterSprites("the sheet must stay an 8-bit indexed PNG".into())),
    };
    let (width, height) = (picture.width as usize, picture.height as usize);
    if (width, height) != (json.meta.size.w, json.meta.size.h) {
        return Err(Error::CharacterSprites(format!("the PNG is {width}x{height}; the JSON names {}x{}", json.meta.size.w, json.meta.size.h)));
    }

    let extension = &json.meta.bof3;
    let mut stages = Stages::new(section, extension.palette)?;
    let mut out = section.clone();

    let palette = &mut out.palettes[extension.palette];
    for (index, colour) in palette.iter_mut().enumerate() {
        let picked = *picture.colors.get(index).ok_or_else(|| Error::CharacterSprites(format!("the PNG palette has {} colours; a palette has {COLOURS}", picture.colors.len())))?;
        if picked != word_to_view(*colour) {
            *colour = view_to_word(picked);
        }
    }

    let left = -extension.origin.x;
    let top = -extension.origin.y;
    let bounds = Bounds { left, top, right: left + extension.cell.w as i32, bottom: top + extension.cell.h as i32 };
    let mut changes: BTreeMap<(usize, u8, u8), (u8, String)> = BTreeMap::new();
    for pose in &extension.poses {
        let animation = section.animations.get(pose.animation).ok_or_else(|| missing("animation", pose.animation))?;
        let frame = animation.frames.get(pose.frame).ok_or_else(|| missing("frame", pose.frame))?;
        let sheet = section.image_of(pose.animation).ok_or_else(|| Error::CharacterSprites(format!("no record plays animation {}", pose.animation)))?;
        let composite = compose(stages.get(sheet), frame, sizes, false, bounds);
        let place = format!("animation {} frame {}", pose.animation, pose.frame);

        for line in 0..bounds.height() {
            for dot in 0..bounds.width() {
                let (x, y) = (pose.x + dot, pose.y + line);
                let index = *picture.indices.get(y * width + x).filter(|_| x < width).ok_or_else(|| Error::CharacterSprites(format!("{place} runs past the PNG")))?;
                match composite.pixel(dot, line) {
                    Some(pixel) if pixel.texel.index != index => {
                        let key = (sheet, pixel.u, pixel.v);
                        if let Some((other, by)) = changes.get(&key).filter(|(other, _)| *other != index) {
                            return Err(Error::CharacterSprites(format!("sheet {sheet} texel ({}, {}) is {other} in {by} and {index} in {place}", pixel.u, pixel.v)));
                        }
                        changes.insert(key, (index, place.clone()));
                    }
                    Some(_) => {}
                    None if index != BACKGROUND => {
                        return Err(Error::CharacterSprites(format!("{place} pixel ({dot}, {line}) is painted where no cell reaches")));
                    }
                    None => {}
                }
            }
        }
    }

    let mut edited: BTreeMap<usize, Vec<u8>> = BTreeMap::new();
    for ((sheet, u, v), (index, _)) in changes {
        let texels = edited.entry(sheet).or_insert_with(|| out.images[sheet].texels().to_vec());
        texels[usize::from(v) * out.images[sheet].texel_width() + usize::from(u)] = index;
    }
    for (sheet, texels) in edited {
        out.images[sheet].set_texels(texels)?;
    }

    let positions: Vec<(usize, usize)> = extension.animations.iter().flat_map(|exported| exported.steps.iter().map(move |&position| (exported.animation, position))).collect();
    if positions.len() != json.frames.len() {
        return Err(Error::CharacterSprites(format!("the JSON has {} frames; the section's animations show {}", json.frames.len(), positions.len())));
    }
    for (&(animation, position), frame) in positions.iter().zip(&json.frames) {
        let step = out.animations.get_mut(animation).and_then(|animation| animation.steps.get_mut(position)).ok_or_else(|| missing("step", position))?;
        if duration_ms(step.delay) != frame.duration {
            step.delay = delay_of(frame.duration).ok_or_else(|| {
                Error::CharacterSprites(format!("{} lasts {} ms; a step lasts 1 to {} ticks of {:.3} ms", frame.filename, frame.duration, super::JUMP - 1, TICK_MICROS / MICROS_PER_MILLI))
            })?;
        }
    }
    Ok(out)
}

fn read_json(json: &[u8]) -> Result<Json> {
    serde_json::from_slice(json).map_err(|error| Error::CharacterSprites(format!("the sheet JSON does not read: {error}")))
}

/// The PNG file name a sheet's JSON names (`meta.image`).
pub fn image_name(json: &[u8]) -> Result<String> {
    Ok(read_json(json)?.meta.image)
}

/// The steps of `animation` that show a frame: stream position, frame and delay.
fn shown_steps(animation: &Animation, index: usize) -> Result<Vec<(usize, usize, u8)>> {
    animation
        .shown()
        .map(|(position, step)| {
            let frame = usize::from(step.frame);
            if frame >= animation.frames.len() {
                return Err(Error::CharacterSprites(format!("animation {index} step {position} shows frame {frame} of {}", animation.frames.len())));
            }
            Ok((position, frame, step.delay))
        })
        .collect()
}

/// The PNG palette: the section palette's colours, then the marker.
fn png_colours(palette: &super::Palette) -> Vec<[u8; 4]> {
    let mut colours: Vec<[u8; 4]> = palette.iter().map(|&word| word_to_view(word)).collect();
    colours.resize(PNG_COLOURS, MARKER);
    colours
}

fn missing(what: &str, index: usize) -> Error {
    Error::CharacterSprites(format!("the sheet names {what} {index}, which the section does not have"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sprite::{SHAPES, tests::section_bytes};

    fn sizes() -> SpriteSizes {
        let mut rows = [[0; 2]; SHAPES];
        for (shape, row) in rows.iter_mut().enumerate() {
            *row = [8 * (shape as u8 % 4 + 1), 8 * (shape as u8 / 4 + 1)];
        }
        SpriteSizes::from_rows(rows)
    }

    fn section() -> Section {
        Section::decode(&section_bytes(1, 0x400)).expect("decodes")
    }

    #[test]
    fn a_bank_sheet_names_its_section() {
        let sprites = crate::sprite::decode(&crate::sprite::tests::container(0x400)).expect("decodes");
        let sheet = export(&sprites, 2, 0, &sizes(), "sprites.png").expect("exports");
        assert_eq!(image_name(&sheet.json).expect("names its PNG"), "sprites.png");
        let imported = import(&sprites, &sizes(), &sheet.json, &sheet.png).expect("imports");
        assert_eq!(crate::sprite::encode(&imported).expect("encodes"), crate::sprite::encode(&sprites).expect("encodes"));
        assert!(export(&sprites, 3, 0, &sizes(), "sprites.png").is_err());
    }

    #[test]
    fn durations_round_trip_through_milliseconds() {
        for delay in (0..super::super::JUMP).chain([0]) {
            assert_eq!(delay_of(duration_ms(delay)), Some(delay), "delay {delay}");
        }
        assert_eq!(duration_ms(1), 33);
        assert_eq!(delay_of(0), None);
    }

    #[test]
    fn an_unchanged_sheet_imports_to_the_stored_section() {
        let section = section();
        let sheet = export_section(&section, 0, 0, &sizes(), "sprites.png").expect("exports");
        let imported = import_section(&section, &sizes(), &sheet.json, &sheet.png).expect("imports");
        assert_eq!(imported.encode().expect("encodes"), section.encode().expect("encodes"));
        assert!(imported.images.iter().all(super::super::Image::is_stored));
    }

    #[test]
    fn the_json_is_aseprite_shaped() {
        let sheet = export_section(&section(), 0, 0, &sizes(), "sprites.png").expect("exports");
        let json: serde_json::Value = serde_json::from_slice(&sheet.json).expect("JSON");
        assert_eq!(json["meta"]["image"], "sprites.png");
        assert_eq!(json["meta"]["frameTags"][0]["direction"], "forward");
        // Animation 0 shows two steps (the jump is not a frame), animation 1 one.
        assert_eq!(json["frames"].as_array().map(Vec::len), Some(3));
        assert_eq!(json["frames"][1]["duration"], duration_ms(6));
        assert_eq!(json["meta"]["bof3"]["animations"][0]["loop_to"], 0);
    }

    /// The PNG with one pixel of pose `pose` set to `index`.
    fn repaint(sheet: &Sheet, pose: usize, dot: (usize, usize), index: u8) -> Vec<u8> {
        let json: serde_json::Value = serde_json::from_slice(&sheet.json).expect("JSON");
        let Ok(Picture::Indexed(mut picture)) = png::read(&sheet.png) else { panic!("an indexed PNG") };
        let pose = &json["meta"]["bof3"]["poses"][pose];
        let (x, y) = (pose["x"].as_u64().unwrap() as usize + dot.0, pose["y"].as_u64().unwrap() as usize + dot.1);
        picture.indices[y * picture.width as usize + x] = index;
        png::indexed_png(picture.width, picture.height, PNG_BITS, &picture.colors, &picture.indices)
    }

    #[test]
    fn a_painted_pixel_goes_to_its_sheet_texel() {
        let section = section();
        let sheet = export_section(&section, 0, 0, &sizes(), "sprites.png").expect("exports");
        // Pose 0 is animation 0 frame 0: one 16x16 cell at (-8, -8) over texels (0..16, 0..8); the sheet is
        // 8 rows tall, so rows 8-15 of the cell are outside it. The box starts at (-8, -8) too.
        let png = repaint(&sheet, 0, (3, 2), 30);
        let imported = import_section(&section, &sizes(), &sheet.json, &png).expect("imports");
        assert_eq!(imported.images[0].texel(3, 2), Some(30));
        let changed = (0..imported.images[0].texel_count()).filter(|&at| imported.images[0].texels()[at] != section.images[0].texels()[at]).count();
        assert_eq!(changed, 1);
        assert!(imported.encode().is_ok());
    }

    #[test]
    fn two_different_changes_to_one_texel_are_refused() {
        let section = section();
        let sheet = export_section(&section, 0, 0, &sizes(), "sprites.png").expect("exports");
        // Poses 0 (frame 0) and 1 (frame 1) both show texel (0, 0) at their top-left pixel.
        let once = repaint(&sheet, 0, (0, 0), 20);
        let twice = Sheet { png: once, json: sheet.json.clone() };
        let png = repaint(&twice, 1, (0, 0), 21);
        let error = import_section(&section, &sizes(), &sheet.json, &png).expect_err("a conflict");
        assert!(error.to_string().contains("texel (0, 0)"), "{error}");
    }

    #[test]
    fn a_changed_duration_sets_the_delay() {
        let section = section();
        let sheet = export_section(&section, 0, 0, &sizes(), "sprites.png").expect("exports");
        let mut json: serde_json::Value = serde_json::from_slice(&sheet.json).expect("JSON");
        json["frames"][0]["duration"] = serde_json::Value::from(duration_ms(10));
        let imported = import_section(&section, &sizes(), json.to_string().as_bytes(), &sheet.png).expect("imports");
        assert_eq!(imported.animations[0].steps[0].delay, 10);
        assert_eq!(imported.animations[0].steps[1], section.animations[0].steps[1]);
    }

    #[test]
    fn a_changed_palette_colour_comes_back() {
        let section = section();
        let sheet = export_section(&section, 0, 0, &sizes(), "sprites.png").expect("exports");
        let Ok(Picture::Indexed(mut picture)) = png::read(&sheet.png) else { panic!("an indexed PNG") };
        picture.colors[4] = word_to_view(0x7C00);
        let png = png::indexed_png(picture.width, picture.height, PNG_BITS, &picture.colors, &picture.indices);
        let imported = import_section(&section, &sizes(), &sheet.json, &png).expect("imports");
        assert_eq!(imported.palettes[0][4], 0x7C00);
        assert_eq!(imported.palettes[0][5], section.palettes[0][5]);
    }
}
