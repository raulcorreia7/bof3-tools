//! What the area view reads and draws, without the window: the area's decoded entries, the render scene built from
//! them, the cameras and the edits.

use rebof3_emi::replay::Vram;
use rebof3_emi::texture::scene::resolve;
use rebof3_formats::encounter::{ENCOUNTER_BYTES, ENCOUNTER_ENEMIES, EncounterBlock, put_encounter};
use rebof3_formats::field::setup::{self, Placement, Record};
use rebof3_formats::field::{Executable, FACINGS, PROGRAM_ADDRESS, Zone, facing_scripts};
use rebof3_formats::gltf::{self, Node};
use rebof3_formats::map::codes::{battle_meaning, is_exit, meaning_of};
use rebof3_formats::map::draw::{CELL_UNITS, Face, GAME_CAMERA, Quad, Tile, TileField, actor_position, ground, quads};
use rebof3_formats::map::terrain::{self, HEIGHT_SCALE};
use rebof3_formats::map::{CELL_BYTES, MapBundle};
use rebof3_formats::scene::{SceneRecord, SceneTable};
use rebof3_render::math::{self, rot_matrix};
use rebof3_render::{Blend, Camera, Depth, Primitive, Scene, Texture};
use serde_json::json;

/// The load addresses of the area entries the view reads beside the bundle: the packed battle grid (`func_801C6FD4`),
/// the scene table (`Area_FindSceneRecord`) and the encounter block (`Area_PickEncounter`).
pub const BATTLE_GRID: u32 = 0x8003_2000;
pub const SCENE_TABLE: u32 = 0x800E_3800;
pub const ENCOUNTERS: u32 = 0x800E_4000;

/// How far an overlay floats above the terrain, in renderer units, so it does not fight the tiles.
const OVERLAY_LIFT: f32 = 2.0;
/// An actor marker's width and height, and its facing arrow's length and half width, in renderer units.
const MARKER_WIDTH: f32 = 48.0;
const MARKER_HEIGHT: f32 = 96.0;
const ARROW_LENGTH: f32 = 80.0;
const ARROW_HALF: f32 = 16.0;
/// The alpha of a cell overlay and of an actor marker.
const OVERLAY_ALPHA: u8 = 110;
const MARKER_ALPHA: u8 = 220;
/// The colours the view adds: the frame's background, an exit, and what is picked.
pub const BACKGROUND: [u8; 4] = [0x16, 0x18, 0x1E, 0xFF];
const EXIT_COLOUR: [u8; 4] = [0xFF, 0x40, 0x40, 0xC0];
const PICKED_COLOUR: [u8; 4] = [0xFF, 0xE6, 0x50, 0x90];
/// The near plane of the game camera, in view units.
const GAME_NEAR: f32 = 16.0;
/// The horizontal field of view of the free camera, in radians.
const ORBIT_FOV: f32 = 0.9;
/// What a pick id carries in its top byte, and the index below it.
const PICK_TERRAIN: u32 = 1 << 24;
const PICK_ACTOR: u32 = 2 << 24;
const PICK_INDEX: u32 = (1 << 24) - 1;
/// One cell of a 16.16 facing step.
const FIXED_ONE: f32 = 65536.0;
/// The empty slot of an encounter row.
pub const EMPTY_SLOT: u8 = 0xFF;

/// What the view shows over the terrain.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct Layers {
    pub terrain: bool,
    pub exits: bool,
    pub codes: bool,
    pub battle: bool,
    pub zones: bool,
    pub actors: bool,
}

impl Default for Layers {
    fn default() -> Self {
        Self { terrain: true, exits: true, codes: false, battle: false, zones: false, actors: true }
    }
}

/// One record of the setup stream that places an actor, with the scene record its id names.
#[derive(Clone, Debug)]
pub struct Actor {
    pub record: Record,
    pub placement: Placement,
    pub scene: Option<SceneRecord>,
}

/// The entries of one area the view reads.
pub struct Sources<'a> {
    pub bundle: &'a [u8],
    /// Every entry of the archive, by index: its load address and current payload.
    pub entries: Vec<(u32, &'a [u8])>,
    /// The area's index in the executable's tables, when the archive is an area.
    pub area: Option<usize>,
    /// `SLUS_004.22`, and the game code image of `ETC/GAME.EMI`, when the disc has them.
    pub exe: Option<&'a [u8]>,
    pub game_code: Option<&'a [u8]>,
    /// The VRAM of the area's scene and the scene's name, when a scene replays it.
    pub vram: Option<(&'a Vram, &'a str)>,
}

/// Everything the view decoded from one area.
pub struct Area {
    pub bundle: MapBundle,
    pub quads: Vec<Quad>,
    /// The VRAM words the tiles read; empty when no scene replays the area.
    pub vram: Vec<u16>,
    pub scene_name: Option<String>,
    /// Drawn quads whose page or CLUT row the replay does not write.
    pub unwritten: usize,
    /// The battle grid entry, the area program entry and the encounter block entry: index and bytes.
    pub battle: Option<(usize, Vec<u8>)>,
    pub program: Option<(usize, Vec<u8>)>,
    pub encounters: Option<(usize, Vec<u8>)>,
    pub scenes: Vec<SceneRecord>,
    /// The setup stream's actors, or why they cannot be read.
    pub actors: Result<Vec<Actor>, String>,
    pub zones: Result<Vec<Zone>, String>,
    /// `g_FacingSteps`, in 16.16 cells, and `D_801C839C`.
    pub facings: Option<[[i32; 2]; FACINGS]>,
    pub facing_scripts: Option<[[u8; 2]; FACINGS]>,
}

impl Area {
    /// Decodes an area from its entries.
    pub fn read(sources: &Sources<'_>) -> Result<Self, String> {
        let bundle = MapBundle::decode(sources.bundle).map_err(|error| error.to_string())?;
        let quads = quads(&bundle);
        let entry = |address: u32| {
            sources.entries.iter().position(|&(load, _)| load == address).map(|index| (index, sources.entries[index].1))
        };

        let (vram, scene_name, unwritten) = match sources.vram {
            Some((vram, name)) => {
                let unwritten = quads
                    .iter()
                    .filter_map(|quad| quad.tile)
                    .filter(|tile| resolve(vram, tile.page[0], tile.page[1], tile.clut, tile.depth).is_none())
                    .count();
                (vram.words().to_vec(), Some(name.to_string()), unwritten)
            }
            None => (Vec::new(), None, 0),
        };

        let exe = match sources.exe.map(Executable::new) {
            Some(Ok(exe)) => Ok(exe),
            Some(Err(error)) => Err(error.to_string()),
            None => Err("SLUS_004.22 is not in the disc folder".to_string()),
        };
        let area = sources.area.ok_or_else(|| "the archive is not an area of g_AreaLoads".to_string());
        let program = entry(PROGRAM_ADDRESS);
        let scenes = entry(SCENE_TABLE)
            .and_then(|(_, bytes)| SceneTable::decode(bytes).ok())
            .map_or(Vec::new(), |table| table.records().to_vec());
        let actors = match (&exe, &area, program) {
            (Err(error), _, _) | (_, Err(error), _) => Err(error.clone()),
            (_, _, None) => Err(format!("the archive has no area program at {PROGRAM_ADDRESS:#010X}")),
            (Ok(exe), Ok(area), Some((_, program))) => read_actors(exe, *area, program, &scenes),
        };
        let zones = match (&exe, &area) {
            (Ok(exe), Ok(area)) => exe.zones(*area).map_err(|error| error.to_string()),
            (Err(error), _) | (_, Err(error)) => Err(error.clone()),
        };
        let owned = |found: Option<(usize, &[u8])>| found.map(|(index, bytes)| (index, bytes.to_vec()));
        Ok(Self {
            bundle,
            quads,
            vram,
            scene_name,
            unwritten,
            battle: owned(entry(BATTLE_GRID)),
            program: owned(program),
            encounters: owned(entry(ENCOUNTERS)),
            scenes,
            actors,
            zones,
            facings: exe.as_ref().ok().and_then(|exe| exe.facing_steps().ok()),
            facing_scripts: sources.game_code.and_then(|code| facing_scripts(code).ok()),
        })
    }

    pub fn width(&self) -> usize {
        usize::from(self.bundle.width)
    }

    pub fn height(&self) -> usize {
        usize::from(self.bundle.height)
    }

    /// The column and row of a cell.
    pub fn column_row(&self, cell: usize) -> (usize, usize) {
        let width = self.width().max(1);
        (cell % width, cell / width)
    }

    /// The model-space centre of the map, at its mean corner height.
    pub fn centre(&self) -> [f32; 3] {
        let heights: i64 = self.bundle.cells.iter().flat_map(|cell| cell.heights).map(i64::from).sum();
        let mean = heights as f32 / (self.bundle.cells.len().max(1) * CELL_BYTES) as f32;
        let unit = CELL_UNITS as f32;
        [self.width() as f32 * unit / 2.0, mean * HEIGHT_SCALE as f32, self.height() as f32 * unit / 2.0]
    }

    /// The battle-grid value of a cell: two cells a byte, the even one in the high nibble.
    pub fn battle_value(&self, cell: usize) -> Option<u8> {
        let (_, grid) = self.battle.as_ref()?;
        let byte = *grid.get(cell >> 1)?;
        Some(if cell & 1 == 1 { byte & 0x0F } else { byte >> 4 })
    }

    /// The zones that hold a cell, in scan order; `func_801B3C3C` takes the first.
    pub fn zones_at(&self, cell: usize) -> Vec<(usize, Zone)> {
        let (column, row) = self.column_row(cell);
        let (Ok(x), Ok(y)) = (u8::try_from(column), u8::try_from(row)) else { return Vec::new() };
        self.zones.as_ref().map_or(Vec::new(), |zones| {
            zones.iter().copied().enumerate().filter(|(_, zone)| zone.holds(x, y)).collect()
        })
    }

    /// The encounter block, when the area ships one that parses.
    pub fn encounter_block(&self) -> Option<EncounterBlock> {
        EncounterBlock::parse(&self.encounters.as_ref()?.1).ok()
    }

    /// The actors, or none when the stream cannot be read.
    pub fn actor_list(&self) -> &[Actor] {
        self.actors.as_deref().unwrap_or(&[])
    }
}

/// Walks the area's setup stream (`g_AreaLoads[area]` word 0) and joins each placing record to the scene record its
/// id names.
fn read_actors(
    exe: &Executable<'_>,
    area: usize,
    program: &[u8],
    scenes: &[SceneRecord],
) -> Result<Vec<Actor>, String> {
    let offset = |address: u32| address.checked_sub(PROGRAM_ADDRESS).map(|offset| offset as usize);
    let load = exe.area_load(area).map_err(|error| error.to_string())?;
    let at = offset(load)
        .filter(|&at| at + 4 <= program.len())
        .ok_or_else(|| format!("g_AreaLoads[{area}] = {load:#010X} is outside the area program"))?;
    let start = u32::from_le_bytes([program[at], program[at + 1], program[at + 2], program[at + 3]]);
    let start = offset(start).ok_or_else(|| format!("the setup stream {start:#010X} is outside the area program"))?;
    let stream = setup::walk(program, start).map_err(|error| error.to_string())?;
    Ok(stream
        .records
        .into_iter()
        .filter_map(|record| {
            let placement = record.placement(program)?;
            let scene = placement.scene.and_then(|id| scenes.iter().find(|scene| scene.id == id).copied());
            Some(Actor { record, placement, scene })
        })
        .collect())
}

/// The camera the view looks through.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum View {
    /// The game's own camera ([`GAME_CAMERA`]) on a point in half cells.
    Game { x2: u16, y2: u16 },
    /// A free camera turning about a model point.
    Orbit { target: [f32; 3], yaw: f32, pitch: f32, distance: f32 },
}

impl View {
    /// A free camera that sees the whole map.
    pub fn overview(area: &Area) -> Self {
        let span = area.width().max(area.height()) as f32 * CELL_UNITS as f32;
        Self::Orbit { target: area.centre(), yaw: std::f32::consts::FRAC_PI_4, pitch: 0.8, distance: span * 0.9 }
    }

    /// The game's camera on the map's centre.
    pub fn game(area: &Area) -> Self {
        Self::Game { x2: area.bundle.width.into(), y2: area.bundle.height.into() }
    }

    /// The camera transform.
    pub fn camera(&self, area: &Area) -> Camera {
        match *self {
            View::Orbit { target, yaw, pitch, distance } => Camera::orbit(target, yaw, pitch, distance, ORBIT_FOV),
            View::Game { x2, y2 } => {
                let model_to_gte = [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]];
                let rotation = math::multiply(&rot_matrix(GAME_CAMERA.angle), &model_to_gte);
                let target = actor_position(&area.bundle, x2, y2).map(|value| value as f32);
                let mut translation = math::scale(math::apply(&rotation, target), -1.0);
                translation[2] += GAME_CAMERA.distance as f32;
                Camera {
                    rotation,
                    translation,
                    focal: GAME_CAMERA.focal as f32,
                    frame_width: (2 * GAME_CAMERA.offset[0]) as f32,
                    near: GAME_NEAR,
                }
            }
        }
    }
}

/// What a pick names.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Hit {
    /// A terrain quad, by its index in [`Area::quads`].
    Quad(usize),
    /// An actor, by its index in [`Area::actor_list`].
    Actor(usize),
}

impl Hit {
    /// The hit a pick id names.
    pub fn of_id(id: u32) -> Option<Self> {
        let index = (id & PICK_INDEX) as usize;
        match id & !PICK_INDEX {
            PICK_TERRAIN => Some(Hit::Quad(index)),
            PICK_ACTOR => Some(Hit::Actor(index)),
            _ => None,
        }
    }
}

/// The render scene of an area: the terrain, the shown cell layers and the actor markers.
pub fn scene(area: &Area, layers: Layers, camera: &Camera, picked: Option<Hit>) -> Scene {
    let mut primitives = Vec::with_capacity(area.quads.len() * 2);
    for (index, quad) in area.quads.iter().enumerate() {
        let tile = quad.tile.filter(|_| layers.terrain && !area.vram.is_empty());
        primitives.push(terrain(quad, tile, PICK_TERRAIN | index as u32));
    }

    for quad in area.quads.iter().filter(|quad| quad.face == Face::Top) {
        if let Some(colour) = cell_colour(area, layers, quad.cell) {
            primitives.push(overlay(quad, colour));
        }
    }

    if layers.actors {
        for (index, actor) in area.actor_list().iter().enumerate() {
            primitives.extend(marker(area, actor, camera, index, picked == Some(Hit::Actor(index))));
        }
    }

    if let Some(Hit::Quad(index)) = picked
        && let Some(quad) = area.quads.get(index)
    {
        primitives.push(Primitive { depth_test: false, ..overlay(quad, PICKED_COLOUR) });
    }
    Scene { vram: area.vram.clone(), images: Vec::new(), primitives, background: BACKGROUND }
}

/// A terrain quad: textured with its tile, or flat grey when the tile is not drawn.
fn terrain(quad: &Quad, tile: Option<Tile>, id: u32) -> Primitive {
    let corners = quad.corners.map(|corner| corner.map(|value| value as f32));
    match tile {
        Some(tile) => Primitive {
            corners,
            texture: vram_texture(&tile),
            colour: [tile.shade, tile.shade, tile.shade, 0xFF],
            blend: tile.blend.map_or(Blend::Opaque, |blend| Blend::Psx(blend.mode())),
            depth_test: true,
            id: Some(id),
        },
        None => Primitive {
            corners,
            texture: Texture::None,
            colour: untextured(quad),
            blend: Blend::Opaque,
            depth_test: true,
            id: Some(id),
        },
    }
}

/// The colour the shown layers give a cell, when one of them has something to say about it.
fn cell_colour(area: &Area, layers: Layers, cell: usize) -> Option<[u8; 4]> {
    let code = area.bundle.codes.get(cell).copied().unwrap_or(0);
    if layers.exits && is_exit(code) {
        return Some(EXIT_COLOUR);
    }
    if layers.codes && code != 0 {
        return Some(code_colour(code));
    }
    if layers.battle
        && let Some(value) = area.battle_value(cell).filter(|&value| value != 0)
    {
        return Some(battle_colour(value));
    }
    if layers.zones {
        return area.zones_at(cell).first().map(|(index, zone)| zone_colour(*index, zone));
    }
    None
}

/// The VRAM texture of a tile.
fn vram_texture(tile: &Tile) -> Texture {
    let depth = match tile.depth {
        psx_formats::gfx::texture::Depth::Four => Depth::Four,
        psx_formats::gfx::texture::Depth::Eight => Depth::Eight,
    };
    Texture::Vram { page: tile.page, clut: [tile.clut.x, tile.clut.y], depth, uvs: tile.uvs }
}

/// The grey of a quad drawn without its texture: by face, lighter for a higher cell.
fn untextured(quad: &Quad) -> [u8; 4] {
    let height = quad.corners.iter().map(|corner| corner[1]).sum::<i32>() / quad.corners.len() as i32;
    let base: i32 = match quad.face {
        Face::Top => 150,
        Face::East => 90,
        Face::South => 110,
    };
    let value = (base + height / HEIGHT_SCALE).clamp(30, 230) as u8;
    [value, value, value, 0xFF]
}

/// A top quad lifted a little and filled with `colour`.
fn overlay(quad: &Quad, colour: [u8; 4]) -> Primitive {
    Primitive {
        corners: quad.corners.map(|[x, y, z]| [x as f32, y as f32 + OVERLAY_LIFT, z as f32]),
        texture: Texture::None,
        colour,
        blend: Blend::Alpha,
        depth_test: true,
        id: None,
    }
}

/// A hue per high nibble of a byte code; a code no reader tests is fainter.
fn code_colour(code: u8) -> [u8; 4] {
    let [red, green, blue] = hue(f32::from(code >> 4) / 16.0);
    let alpha = if meaning_of(code).is_some() { OVERLAY_ALPHA + 60 } else { OVERLAY_ALPHA / 2 };
    [red, green, blue, alpha]
}

/// A hue per battle reach class (`1`-`4`, `5`-`8`, `9`-`12`, and above).
fn battle_colour(value: u8) -> [u8; 4] {
    let [red, green, blue] = hue(f32::from((value - 1) / 4) / 4.0 + 0.1);
    [red, green, blue, OVERLAY_ALPHA]
}

/// A hue per zone row, fainter for a zone of rate 0.
fn zone_colour(index: usize, zone: &Zone) -> [u8; 4] {
    let [red, green, blue] = hue((index as f32 * 0.17).fract());
    let alpha = if zone.rate == 0 { OVERLAY_ALPHA / 2 } else { OVERLAY_ALPHA };
    [red, green, blue, alpha]
}

/// A saturated colour at hue `turn` (0 to 1).
fn hue(turn: f32) -> [u8; 3] {
    let sector = turn.fract() * 6.0;
    let rising = (sector.fract() * 255.0) as u8;
    let falling = 255 - rising;
    match sector as u32 {
        0 => [255, rising, 0],
        1 => [falling, 255, 0],
        2 => [0, 255, rising],
        3 => [0, falling, 255],
        4 => [rising, 0, 255],
        _ => [255, 0, falling],
    }
}

/// An actor's marker: a board that faces the camera, and an arrow on the ground along its facing.
fn marker(area: &Area, actor: &Actor, camera: &Camera, index: usize, picked: bool) -> Vec<Primitive> {
    let base = actor_position(&area.bundle, actor.placement.x2, actor.placement.y2).map(|value| value as f32);
    // The camera's rows are its right and down axes in model space.
    let right = math::scale(camera.rotation[0], MARKER_WIDTH / 2.0);
    let up = math::scale(camera.rotation[1], -MARKER_HEIGHT);
    let corners = [
        math::add(math::sub(base, right), up),
        math::add(math::add(base, right), up),
        math::sub(base, right),
        math::add(base, right),
    ];
    let [red, green, blue] = hue(f32::from(actor.record.kind) / 15.0);
    let colour = if picked {
        [PICKED_COLOUR[0], PICKED_COLOUR[1], PICKED_COLOUR[2], 0xFF]
    } else {
        [red, green, blue, MARKER_ALPHA]
    };
    let mut primitives = vec![Primitive {
        corners,
        texture: Texture::None,
        colour,
        blend: Blend::Alpha,
        depth_test: true,
        id: Some(PICK_ACTOR | index as u32),
    }];
    if let (Some(facing), Some(steps)) = (actor.placement.facing, area.facings.as_ref())
        && let Some(&[step_x, step_y]) = steps.get(usize::from(facing))
        && (step_x, step_y) != (0, 0)
    {
        let direction = math::normalize([step_x as f32 / FIXED_ONE, 0.0, step_y as f32 / FIXED_ONE]);
        let side = [-direction[2] * ARROW_HALF, 0.0, direction[0] * ARROW_HALF];
        let root = math::add(base, [0.0, 2.0 * OVERLAY_LIFT, 0.0]);
        let tip = math::add(root, math::scale(direction, ARROW_LENGTH));
        primitives.push(Primitive {
            corners: [math::sub(root, side), math::add(root, side), tip, tip],
            texture: Texture::None,
            colour: [colour[0], colour[1], colour[2], 0xFF],
            blend: Blend::Alpha,
            depth_test: false,
            id: None,
        });
    }
    primitives
}

/// What the hover line says about a hit: the decoded record and the code that reads it.
pub fn describe(area: &Area, hit: Hit) -> Vec<String> {
    match hit {
        Hit::Quad(index) => area.quads.get(index).map_or_else(Vec::new, |quad| describe_quad(area, quad)),
        Hit::Actor(index) => area.actor_list().get(index).map_or_else(Vec::new, |actor| describe_actor(area, actor)),
    }
}

fn describe_quad(area: &Area, quad: &Quad) -> Vec<String> {
    let cell = quad.cell;
    let (column, row) = area.column_row(cell);
    let code = area.bundle.codes.get(cell).copied().unwrap_or(0);
    let mut lines = vec![format!(
        "Cell x {column} y {row}, {} face. Corners {:?}, a step {HEIGHT_SCALE} units (func_801530D0). Grid value {}.",
        quad.face.label(),
        area.bundle.cells[cell].heights,
        area.bundle.grid[cell],
    )];
    if let Some(tile) = quad.tile {
        let blend = tile.blend.map_or(String::new(), |blend| format!(", semi-transparent mode {}", blend.mode()));
        lines.push(format!(
            "Descriptor {} = {:#010X}: page ({}, {}), CLUT ({}, {}), {}, grey {:#04X}{blend}{}{} (func_80155560).",
            tile.index,
            tile.descriptor,
            tile.page[0],
            tile.page[1],
            tile.clut.x,
            tile.clut.y,
            depth_label(&tile),
            tile.shade,
            if tile.sorted { ", depth-sorted" } else { "" },
            if tile.front { ", front slot" } else { "" },
        ));
    }
    lines.push(match meaning_of(code) {
        Some(meaning) => format!(
            "Code {code:#04X}: {} ({}{}).",
            meaning.name,
            meaning.reader,
            if meaning.matched { "" } else { ", draft C" }
        ),
        None => format!("Code {code:#04X}: unknown, no reader of Area_GetCellCode tests it."),
    });
    if let Some(value) = area.battle_value(cell) {
        lines.push(format!("Battle grid {value}: {} (func_801C7518).", battle_meaning(value)));
    }
    if let Some((index, zone)) = area.zones_at(cell).first() {
        lines.push(format!(
            "Zone row {index} at {:#010X}: cells ({}, {}) to ({}, {}), rate {}, kind {}, music {} {} (func_801B3C3C).",
            zone.address,
            zone.first[0],
            zone.first[1],
            zone.last[0],
            zone.last[1],
            zone.rate,
            zone.kind,
            zone.seed,
            zone.pick
        ));
    }
    lines
}

fn depth_label(tile: &Tile) -> &'static str {
    match tile.depth {
        psx_formats::gfx::texture::Depth::Four => "4-bit",
        psx_formats::gfx::texture::Depth::Eight => "8-bit",
    }
}

fn describe_actor(area: &Area, actor: &Actor) -> Vec<String> {
    let program = area.program.as_ref().map_or(&[][..], |(_, bytes)| bytes.as_slice());
    let bytes: Vec<String> = actor.record.bytes(program).iter().map(|byte| format!("{byte:02X}")).collect();
    let mut lines = vec![
        format!(
            "Kind {:X} record at program +{:#06X}, run by {}: {}.",
            actor.record.kind,
            actor.record.at,
            actor.record.layout().handler,
            bytes.join(" ")
        ),
        format!(
            "Cell ({}, {}), facing {}, scene id {}.",
            half(actor.placement.x2),
            half(actor.placement.y2),
            actor.placement.facing.map_or("none".into(), |facing| facing.to_string()),
            actor.placement.scene.map_or("none".into(), |id| format!("{id:#06X}")),
        ),
    ];
    match (actor.scene, actor.placement.scene) {
        (Some(scene), _) => lines.push(format!(
            "Scene record: page {}, row {}, CLUT select {:#06X}, mode {}, script section {} (Area_FindSceneRecord).",
            scene.texture_page, scene.texture_v, scene.clut_select, scene.texture_mode, scene.script_section
        )),
        (None, Some(_)) => lines.push("Scene record: unknown, the area's scene table has no row with this id.".into()),
        (None, None) => {}
    }
    if let (Some(facing), Some(scripts)) = (actor.placement.facing, area.facing_scripts.as_ref())
        && let Some([script, flip]) = scripts.get(usize::from(facing))
    {
        lines.push(format!("Facing {facing} starts script {script}, flip {flip} (D_801C839C, func_8015BAC4)."));
    }
    if !actor.record.guards.is_empty() {
        let guards: Vec<String> = actor.record.guards.iter().map(|guard| guard.label()).collect();
        lines.push(format!("Runs under {}.", guards.join(", then ")));
    }
    lines.push("Drawn as a marker: the field sprite draw (Area_DrawObjectSprite) is not decoded yet.".into());
    lines
}

/// A half-cell coordinate as text: `7` is `3.5`.
pub fn half(value: u16) -> String {
    if value & 1 == 1 { format!("{}.5", value / 2) } else { (value / 2).to_string() }
}

/// A half-cell coordinate from text: a whole number or one ending in `.5`.
pub fn parse_half(text: &str) -> Option<u16> {
    let text = text.trim();
    match text.strip_suffix(".5") {
        Some(whole) => whole.parse::<u16>().ok()?.checked_mul(2)?.checked_add(1),
        None => text.parse::<u16>().ok()?.checked_mul(2),
    }
}

/// The bundle with corner `corner` of `cell` set to `height`.
pub fn with_corner(payload: &[u8], cell: usize, corner: usize, height: i8) -> Result<Vec<u8>, String> {
    edit_bundle(payload, |bundle| {
        let record = bundle.cells.get_mut(cell).ok_or("no such cell")?;
        *record.heights.get_mut(corner).ok_or("no such corner")? = height;
        Ok(())
    })
}

/// The bundle with every corner of `cell` moved by `steps`.
pub fn with_raised(payload: &[u8], cell: usize, steps: i8) -> Result<Vec<u8>, String> {
    edit_bundle(payload, |bundle| {
        let record = bundle.cells.get_mut(cell).ok_or("no such cell")?;
        for height in &mut record.heights {
            *height = height.checked_add(steps).ok_or("a corner height is a signed byte, -128 to 127")?;
        }
        Ok(())
    })
}

/// The bundle with byte code `code` at `cell`.
pub fn with_code(payload: &[u8], cell: usize, code: u8) -> Result<Vec<u8>, String> {
    edit_bundle(payload, |bundle| {
        let cells = bundle.cells.len();
        *bundle.codes.get_mut(cell).filter(|_| cell < cells).ok_or("no such cell")? = code;
        Ok(())
    })
}

/// The bundle with `field` of descriptor `index` set to `value`.
pub fn with_tile_field(payload: &[u8], index: usize, field: TileField, value: u32) -> Result<Vec<u8>, String> {
    let mut bundle = MapBundle::decode(payload).map_err(|error| error.to_string())?;
    let word = bundle.records.get_mut(index).ok_or("no such descriptor")?;
    *word = field.set(*word, value).ok_or_else(|| {
        let (_, width) = field.bits();
        format!("{} takes 0 to {}", field.label(), (1u32 << width) - 1)
    })?;
    Ok(bundle.encode())
}

/// Decodes the bundle, edits it, and encodes it.
fn edit_bundle(
    payload: &[u8],
    edit: impl FnOnce(&mut MapBundle) -> Result<(), &'static str>,
) -> Result<Vec<u8>, String> {
    let mut bundle = MapBundle::decode(payload).map_err(|error| error.to_string())?;
    edit(&mut bundle).map_err(str::to_string)?;
    Ok(bundle.encode())
}

/// How many drawn quads name descriptor `index`.
pub fn sharers(area: &Area, index: usize) -> usize {
    area.quads.iter().filter(|quad| quad.tile.is_some_and(|tile| tile.index == index)).count()
}

/// The battle grid with `cell` set to `value` (`0` to `15`, a nibble).
pub fn with_battle(grid: &[u8], cell: usize, value: u8) -> Result<Vec<u8>, String> {
    if value > 0x0F {
        return Err("a battle grid value is a nibble, 0 to 15".into());
    }
    let mut grid = grid.to_vec();
    let byte = grid.get_mut(cell >> 1).ok_or("the cell is past the battle grid")?;
    *byte = if cell & 1 == 1 { (*byte & 0xF0) | value } else { (*byte & 0x0F) | (value << 4) };
    Ok(grid)
}

/// The area program with actor `actor` at `x2`, `y2` half cells, inside the map.
pub fn with_actor_position(area: &Area, actor: usize, x2: u16, y2: u16) -> Result<Vec<u8>, String> {
    let (actor, program) = actor_and_program(area, actor)?;
    if usize::from(x2 / 2) >= area.width() || usize::from(y2 / 2) >= area.height() {
        return Err(format!("({}, {}) is outside the {} x {} map", half(x2), half(y2), area.width(), area.height()));
    }
    setup::with_position(program, &actor.record, x2, y2).map_err(|error| error.to_string())
}

/// The area program with actor `actor` facing `facing`.
pub fn with_actor_facing(area: &Area, actor: usize, facing: u8) -> Result<Vec<u8>, String> {
    let (actor, program) = actor_and_program(area, actor)?;
    setup::with_facing(program, &actor.record, facing).map_err(|error| error.to_string())
}

/// The area program with actor `actor` naming scene id `scene`, which must be a row of the area's scene table: the id
/// selects the sprite page, CLUT row and script section.
pub fn with_actor_scene(area: &Area, actor: usize, scene: u16) -> Result<Vec<u8>, String> {
    let (actor, program) = actor_and_program(area, actor)?;
    if !area.scenes.iter().any(|record| record.id == scene) {
        return Err(format!("the area's scene table has no row with id {scene:#06X}"));
    }
    setup::with_scene(program, &actor.record, scene).map_err(|error| error.to_string())
}

fn actor_and_program(area: &Area, actor: usize) -> Result<(&Actor, &[u8]), String> {
    let actor = area.actor_list().get(actor).ok_or("no such actor")?;
    let (_, program) = area.program.as_ref().ok_or("the area has no program")?;
    Ok((actor, program))
}

/// The encounter block with slot `slot` of row `row` set: slots `0` to `7` are enemy indexes ([`EMPTY_SLOT`] for
/// none), slot `8` the appearance rate.
pub fn with_encounter(block: &[u8], row: usize, slot: usize, value: u8) -> Result<Vec<u8>, String> {
    let parsed = EncounterBlock::parse(block).map_err(|error| error.to_string())?;
    let mut encounter = *parsed.encounters.get(row).ok_or("no such encounter row")?;
    if slot < ENCOUNTER_ENEMIES {
        if value != EMPTY_SLOT && usize::from(value) >= parsed.enemies.len() {
            return Err(format!(
                "enemy {value} is past the block's {} enemy records; {EMPTY_SLOT} empties the slot",
                parsed.enemies.len()
            ));
        }
        encounter.enemies[slot] = value;
    } else if slot == ENCOUNTER_ENEMIES {
        encounter.appearance_rate = value;
    } else {
        return Err("no such slot".into());
    }
    let mut block = block.to_vec();
    put_encounter(&mut block[row * ENCOUNTER_BYTES..(row + 1) * ENCOUNTER_BYTES], &encounter)?;
    Ok(block)
}

/// The name of actor `index`'s node in the glTF.
fn actor_node(index: usize) -> String {
    format!("actor {index}")
}

/// The area as a binary glTF: the terrain mesh with its grey levels as vertex colours ([`terrain::glb_of_bundle`]),
/// then named nodes.
pub fn export(area: &Area, name: &str) -> Option<Vec<u8>> {
    let bundle = &area.bundle;
    let mut cells = terrain::cells_node(bundle);
    if area.battle.is_some() {
        let values: Vec<u8> = (0..bundle.cells.len()).filter_map(|cell| area.battle_value(cell)).collect();
        cells.extras["battle"] = json!(values);
        cells.extras["battle_reader"] = json!("func_801C6FD4, func_801C7518");
    }

    let program = area.program.as_ref().map_or(&[][..], |(_, bytes)| bytes.as_slice());
    let actors = area
        .actor_list()
        .iter()
        .enumerate()
        .map(|(index, actor)| {
            let Placement { x2, y2, facing, scene } = actor.placement;
            Node {
                name: actor_node(index),
                translation: Some([
                    (f32::from(x2) + 1.0) / 2.0,
                    ground(bundle, x2, y2) as f32,
                    (f32::from(y2) + 1.0) / 2.0,
                ]),
                extras: json!({
                    "kind": actor.record.kind,
                    "record": actor.record.at,
                    "bytes": actor.record.bytes(program),
                    "handler": actor.record.layout().handler,
                    "x2": x2,
                    "y2": y2,
                    "facing": facing,
                    "scene": scene,
                }),
                children: Vec::new(),
            }
        })
        .collect();

    let zones = area.zones.as_ref().map_or(Vec::new(), |zones| {
        zones
            .iter()
            .enumerate()
            .map(|(index, zone)| Node {
                name: format!("zone {index}"),
                translation: Some([
                    (f32::from(zone.first[0]) + f32::from(zone.last[0]) + 1.0) / 2.0,
                    0.0,
                    (f32::from(zone.first[1]) + f32::from(zone.last[1]) + 1.0) / 2.0,
                ]),
                extras: json!({
                    "first": zone.first, "last": zone.last, "rate": zone.rate, "kind": zone.kind,
                    "seed": zone.seed, "pick": zone.pick, "address": zone.address,
                }),
                children: Vec::new(),
            })
            .collect()
    });
    let formations = area.encounter_block().map_or(json!(null), |block| {
        json!(
            block
                .encounters
                .iter()
                .map(|encounter| json!({"enemies": encounter.enemies, "rate": encounter.appearance_rate}))
                .collect::<Vec<_>>()
        )
    });
    let nodes = [
        cells,
        Node {
            name: "actors".into(),
            extras: json!({"reader": "Area_RunSetupStream (emi/etc/game/00@0x801A3E50)"}),
            children: actors,
            ..Node::default()
        },
        Node {
            name: "encounters".into(),
            extras: json!({
                "formations": formations,
                "zones_reader": "func_801B3C3C (emi/etc/game/00@0x801B3C3C)",
                "formations_reader": "Area_PickEncounter (emi/etc/game/00@0x801C6A7C)",
            }),
            children: zones,
            ..Node::default()
        },
    ];
    terrain::glb_of_bundle(bundle, name, &nodes)
}

/// What a glTF carries back: the bundle with the mesh's corner heights ([`terrain::import`]), and the area program
/// with each actor at its `actor N` node.
pub fn import(area: &Area, bundle: &[u8], glb: &[u8]) -> Result<(Vec<u8>, Option<Vec<u8>>), String> {
    let rebuilt = terrain::import(glb, bundle).map_err(|error| error.to_string())?;
    let Some((_, original)) = &area.program else { return Ok((rebuilt, None)) };
    let mut program = original.clone();
    for (name, translation) in gltf::read_nodes(glb)? {
        let Some(index) = name.strip_prefix("actor ").and_then(|index| index.parse::<usize>().ok()) else { continue };
        let actor = area.actor_list().get(index).ok_or_else(|| format!("{name}: the area has no such actor"))?;
        let [x, _, z] = translation.ok_or_else(|| format!("{name} has no translation"))?;
        let half = |value: f32| {
            let steps = value * 2.0 - 1.0;
            (steps.fract() == 0.0 && (0.0..=f32::from(u16::MAX)).contains(&steps)).then_some(steps as u16)
        };
        let (Some(x2), Some(y2)) = (half(x), half(z)) else {
            return Err(format!("{name} at ({x}, {z}) is not on the half-cell grid"));
        };
        if actor.placement.x2 == x2 && actor.placement.y2 == y2 {
            continue;
        }
        if usize::from(x2 / 2) >= area.width() || usize::from(y2 / 2) >= area.height() {
            return Err(format!("{name} at ({x}, {z}) is outside the map"));
        }
        program = setup::with_position(&program, &actor.record, x2, y2).map_err(|error| error.to_string())?;
    }
    Ok((rebuilt, (program != *original).then_some(program)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rebof3_formats::map::Cell;

    /// A 4 x 4 bundle whose inner cells hold descriptor 1.
    fn bundle() -> MapBundle {
        let mut grid = vec![0; 16];
        for cell in [5, 6, 9, 10] {
            grid[cell] = 1;
        }
        MapBundle {
            width: 4,
            height: 4,
            cells: vec![Cell { heights: [1; CELL_BYTES] }; 16],
            codes: vec![0; 16],
            grid,
            records: vec![0, 0x0080_0000],
            ..MapBundle::default()
        }
    }

    fn area() -> Area {
        let bundle = bundle().encode();
        let sources =
            Sources { bundle: &bundle, entries: Vec::new(), area: None, exe: None, game_code: None, vram: None };
        Area::read(&sources).expect("the area reads")
    }

    #[test]
    fn edits_change_only_their_bytes() {
        let payload = bundle().encode();
        assert_eq!(with_corner(&payload, 5, 0, 1).unwrap(), payload, "an unchanged corner writes the same bytes");
        let raised = MapBundle::decode(&with_raised(&payload, 5, 2).unwrap()).unwrap();
        assert_eq!(raised.cells[5].heights, [3; CELL_BYTES]);
        assert_eq!(raised.cells[6], bundle().cells[6]);
        assert!(with_raised(&payload, 5, 127).is_err(), "1 + 127 leaves the byte");

        let coded = with_code(&payload, 6, 0xA7).unwrap();
        assert_eq!(coded.iter().zip(&payload).filter(|(new, old)| new != old).count(), 1);
        let tinted = MapBundle::decode(&with_tile_field(&payload, 1, TileField::Clut, 2).unwrap()).unwrap();
        assert_eq!(tinted.records[1], 0x0280_0000);
        assert!(with_tile_field(&payload, 1, TileField::Page, 9).is_err());

        assert_eq!(with_battle(&[0x12], 1, 7).unwrap(), [0x17]);
        assert_eq!(with_battle(&[0x12], 0, 7).unwrap(), [0x72]);
        assert!(with_battle(&[0x12], 2, 7).is_err());
        assert!(with_battle(&[0x12], 0, 16).is_err());
        assert_eq!((parse_half("3.5"), parse_half("4"), parse_half("x")), (Some(7), Some(8), None));
    }

    #[test]
    fn the_scene_draws_every_quad_and_picks_them() {
        let area = area();
        assert_eq!(area.quads.len(), 4);
        assert!(area.actors.is_err() && area.zones.is_err(), "no executable, so no actors or zones");
        let camera = View::overview(&area).camera(&area);
        let scene = scene(&area, Layers::default(), &camera, None);
        assert_eq!(scene.primitives.len(), 4);
        let frame = rebof3_render::software::draw(&scene, &camera, 160, 120);
        let drawn = frame.rgba.chunks(4).filter(|pixel| pixel[..3] != BACKGROUND[..3]).count();
        assert!(drawn > 100, "the terrain covers part of the frame ({drawn} pixels)");
        let centre = rebof3_render::pick(&scene, &camera, 160, 120, 80.0, 60.0);
        assert!(matches!(centre.and_then(Hit::of_id), Some(Hit::Quad(_))), "{centre:?}");
    }

    #[test]
    fn the_game_camera_centres_its_point_at_the_view_distance() {
        let area = area();
        let camera = View::Game { x2: 4, y2: 4 }.camera(&area);
        let target = actor_position(&area.bundle, 4, 4).map(|value| value as f32);
        let view = camera.view(target);
        assert!(view[0].abs() < 1e-2 && view[1].abs() < 1e-2, "the point is on the axis: {view:?}");
        assert!((view[2] - GAME_CAMERA.distance as f32).abs() < 1e-2);
        let above = camera.view([target[0], target[1] + 100.0, target[2]]);
        assert!(above[1] < view[1], "a higher point is higher on the screen: {above:?}");
    }
}
