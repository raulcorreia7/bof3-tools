//! The game's own draws as evidence: run the retail disc on a scripted route and record which
//! depth and CLUT row the game draws each texture upload with.
//!
//! `psx-machine` runs the disc's code. A [`Route`] presses the buttons. While it runs, the
//! recorder ([`record`]) follows the files the game reads ([`sources`]), names the EMI entry each
//! upload came from, and joins every textured draw to the entry whose uploads wrote the words it
//! samples. A draw proves the depth of that entry, and its CLUT attribute names the palette row;
//! the entry that wrote the row is named when its words match.
//!
//! An observation is evidence, not a format: it is kept as a cache ([`cache`]) under the
//! project's `build/cache/observe/`, one file per disc and route, and rebuilt when the route, the
//! machine's revision or the file layout changes.

#![forbid(unsafe_code)]

pub mod cache;
pub mod compare;
pub mod record;
pub mod route;
pub mod sources;

use std::collections::BTreeMap;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Instant;

use psx_formats::gfx::texture::{Clut, Depth};
use psx_machine::Machine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use route::Route;

use record::{PaletteKind, Recorder};
use sources::Sources;

/// The bof3-player revision of the machine that made an observation (`Cargo.lock`).
pub const PLAYER_REVISION: &str = env!("BOF3_PLAYER_REVISION");
/// The layout of [`Observation`]; a cache with another number is rebuilt.
pub const FORMAT_VERSION: u32 = 1;
/// Instructions one frame may run before the run counts as stuck: at the machine's two cycles per
/// instruction and 33.8688 MHz, about three seconds of game time.
const FRAME_BUDGET: u64 = 50_000_000;
/// Bytes hashed per read of the disc image.
const HASH_CHUNK: usize = 1 << 20;

/// Why a run or a cache operation failed.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// A route file is not a route.
    Route(String),
    /// The disc image does not open or boot.
    Disc(String),
    /// The game stopped the machine.
    Halt {
        /// The frame it stopped in.
        frame: u64,
        /// What stopped it.
        reason: String,
    },
    /// The caller asked the run to stop.
    Cancelled,
    /// A cache file does not read or write.
    Cache(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Route(message) => write!(f, "route: {message}"),
            Self::Disc(message) => write!(f, "disc: {message}"),
            Self::Halt { frame, reason } => write!(f, "the machine stopped at frame {frame}: {reason}"),
            Self::Cancelled => f.write_str("cancelled"),
            Self::Cache(message) => write!(f, "cache: {message}"),
        }
    }
}

impl std::error::Error for Error {}

/// One way the game drew an entry, as the cache stores it.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Texture {
    /// The archive's disc path (`BIN/BATTLE/BATL_DRA.EMI`).
    pub archive: String,
    /// The entry's slot.
    pub slot: usize,
    /// The page depth: 4, 8 or 15 bits.
    pub bits: u32,
    /// The CLUT row's VRAM column and row, for a 4- or 8-bit page.
    pub clut: Option<[u16; 2]>,
    /// The entry that wrote the CLUT row, when its words match one.
    pub palette: Option<Palette>,
    /// The CLUT row's words as the draw found them.
    pub words: Vec<u16>,
    /// The frame of the first draw.
    pub frame: u64,
    /// The `GP0` command byte of the first draw.
    pub opcode: u8,
    /// Draws seen; a draw repeated in one VRAM state counts once per state it is seen in.
    pub draws: u64,
    /// The VRAM words the draws sampled: left, top, right and bottom, exclusive.
    pub area: [u16; 4],
}

/// The entry that wrote a CLUT row.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Palette {
    /// The archive's disc path.
    pub archive: String,
    /// The entry's slot.
    pub slot: usize,
    /// `upload` (a type-3 entry sent the row) or `arena` (a type-0 entry loaded it into the
    /// palette arena).
    pub via: String,
}

impl Texture {
    /// The texel depth; `None` is 15-bit.
    pub fn depth(&self) -> Option<Depth> {
        match self.bits {
            4 => Some(Depth::Four),
            8 => Some(Depth::Eight),
            _ => None,
        }
    }

    /// The CLUT row.
    pub fn clut(&self) -> Option<Clut> {
        self.clut.map(|[x, y]| Clut { x, y })
    }

    /// One line of evidence: the route, the frame and the draw command.
    pub fn evidence(&self, route: &str) -> String {
        format!("observed: route {route}, frame {}, GP0({:02X}h)", self.frame, self.opcode)
    }
}

/// What one run of a route recorded.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Observation {
    /// [`FORMAT_VERSION`] when it was made.
    pub version: u32,
    /// The route's name.
    pub route: String,
    /// The SHA-256 of the route's text.
    pub route_sha: String,
    /// The SHA-256 of the disc's data track.
    pub disc_sha: String,
    /// [`PLAYER_REVISION`] when it was made.
    pub player_revision: String,
    /// Frames run.
    pub frames: u64,
    /// Seconds the run took.
    pub seconds: f64,
    /// The EMI files the game read, in the order it first read them.
    pub files: Vec<String>,
    /// `GP0(A0h)` uploads.
    pub uploads: u64,
    /// Uploads an entry of a file read holds.
    pub attributed_uploads: u64,
    /// Textured draws.
    pub textured_draws: u64,
    /// Every entry the draws proved, each way it was drawn, most-drawn first.
    pub textures: Vec<Texture>,
}

impl Observation {
    /// The ways entry `slot` of `archive` was drawn, most-drawn first.
    pub fn of<'a>(&'a self, archive: &'a str, slot: usize) -> impl Iterator<Item = &'a Texture> + 'a {
        self.textures.iter().filter(move |texture| texture.archive == archive && texture.slot == slot)
    }

    /// Entries observed, by archive.
    pub fn coverage(&self) -> BTreeMap<&str, usize> {
        let mut entries: BTreeMap<&str, std::collections::BTreeSet<usize>> = BTreeMap::new();
        for texture in &self.textures {
            entries.entry(&texture.archive).or_default().insert(texture.slot);
        }
        entries.into_iter().map(|(archive, slots)| (archive, slots.len())).collect()
    }
}

/// How far a run is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Progress {
    /// Frames run.
    pub frame: u64,
    /// Frames the route lasts.
    pub frames: u64,
}

/// Boots the disc image at `image`, plays `route` and records what the game drew.
///
/// `progress` is called after every frame; returning `false` stops the run with
/// [`Error::Cancelled`].
///
/// # Errors
///
/// [`Error::Disc`] when the image does not boot, [`Error::Halt`] when the game stops the machine,
/// [`Error::Cancelled`] when `progress` says stop.
pub fn run(image: &Path, route: &Route, disc_sha: &str, mut progress: impl FnMut(Progress) -> bool) -> Result<Observation, Error> {
    let started = Instant::now();
    let mut machine = Machine::boot_image(image).map_err(|error| Error::Disc(error.to_string()))?;
    let disc = psx_cd::Image::open(image).map_err(|error| Error::Disc(error.to_string()))?;
    let mut recorder = Recorder::new(Sources::new(disc).map_err(|error| Error::Disc(error.to_string()))?);
    while machine.frame() < route.frames {
        let next = machine.frame() + 1;
        machine.kernel_mut().buttons = route.buttons(machine.frame());
        machine.run_until(|machine| machine.frame() >= next, FRAME_BUDGET).map_err(|halt| Error::Halt { frame: machine.frame(), reason: halt.to_string() })?;
        recorder.sources.follow(&machine.bus().cdrom.commands);
        let log = machine.bus_mut().gpu.take_log();
        recorder.log(&log);
        if !progress(Progress { frame: machine.frame(), frames: route.frames }) {
            return Err(Error::Cancelled);
        }
    }

    let textures = recorder
        .by_entry()
        .into_iter()
        .flat_map(|((archive, slot), uses)| {
            let files = &recorder.sources.files;
            uses.into_iter().map(move |used| Texture {
                archive: archive.clone(),
                slot,
                bits: used.depth.map_or(15, Depth::bits),
                clut: used.clut.map(|clut| [clut.x, clut.y]),
                palette: used.palette.map(|(entry, kind)| Palette {
                    archive: files[entry.file].path.clone(),
                    slot: entry.slot,
                    via: match kind {
                        PaletteKind::Upload => "upload",
                        PaletteKind::Arena => "arena",
                    }
                    .into(),
                }),
                words: used.words,
                frame: used.frame,
                opcode: used.opcode,
                draws: used.draws,
                area: used.area,
            })
        })
        .collect();
    Ok(Observation {
        version: FORMAT_VERSION,
        route: route.name.clone(),
        route_sha: route.sha.clone(),
        disc_sha: disc_sha.to_string(),
        player_revision: PLAYER_REVISION.to_string(),
        frames: machine.frame(),
        seconds: started.elapsed().as_secs_f64(),
        files: recorder.sources.files.iter().map(|file| file.path.clone()).collect(),
        uploads: recorder.uploads.total,
        attributed_uploads: recorder.uploads.attributed,
        textured_draws: recorder.textured_draws,
        textures,
    })
}

/// The variable that names the disc image, as for the retail tests.
pub const IMAGE_VARIABLE: &str = "BOF3_DISC_IMAGE";
/// The folders that hold the disc image below a project root, the ones `bof3 disc extract` searches.
const IMAGE_FOLDERS: [&str; 2] = ["inputs/external", "disk"];

/// The disc image for an extracted disc folder: the one `BOF3_DISC_IMAGE` names, else the image in
/// `inputs/external/` or `disk/` of the nearest folder above `disc` that has one.
///
/// # Errors
///
/// A sentence that says where it looked.
pub fn find_image(disc: &Path) -> Result<PathBuf, String> {
    if let Some(image) = std::env::var_os(IMAGE_VARIABLE).filter(|value| !value.is_empty()) {
        let image = PathBuf::from(image);
        return if image.is_file() { Ok(image) } else { Err(format!("{IMAGE_VARIABLE}={} is not a file", image.display())) };
    }
    for folder in disc.ancestors() {
        for below in IMAGE_FOLDERS {
            if let Ok(image) = psx_cd::discover_image(&folder.join(below)) {
                return Ok(image);
            }
        }
    }
    Err(format!("no disc image: set {IMAGE_VARIABLE} to the .cue sheet, or put it in {} above {}", IMAGE_FOLDERS.join(" or "), disc.display()))
}

/// The SHA-256 of a disc image's data track: the file a `.cue` sheet names for its first data
/// track, or the image file itself.
///
/// # Errors
///
/// [`Error::Disc`] when the sheet or the track does not read.
pub fn disc_sha(image: &Path) -> Result<String, Error> {
    let track = data_track(image)?;
    let mut file = std::fs::File::open(&track).map_err(|error| Error::Disc(format!("{}: {error}", track.display())))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; HASH_CHUNK];
    loop {
        let read = file.read(&mut buffer).map_err(|error| Error::Disc(format!("{}: {error}", track.display())))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(route::hex(&hasher.finalize()))
}

fn data_track(image: &Path) -> Result<PathBuf, Error> {
    let is_sheet = image.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("cue"));
    if !is_sheet {
        return Ok(image.to_path_buf());
    }
    let sheet = psx_cd::cue::read(image).map_err(|error| Error::Disc(format!("{}: {error}", image.display())))?;
    sheet.tracks.iter().find(|track| track.mode != psx_cd::cue::TrackMode::Audio).map(|track| track.file.clone()).ok_or_else(|| Error::Disc(format!("{} names no data track", image.display())))
}
