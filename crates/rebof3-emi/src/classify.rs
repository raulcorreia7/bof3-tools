//! Entry classification: which asset format an EMI entry holds, and what this
//! crate can do with it.
//!
//! Selection reads the entry header (file type plus load address) and one
//! payload property, an all-zero body, which is the reserved-slot shape the
//! asset census counts on its own. Names and addresses come from
//! `wiki: formats/area`, `wiki: formats/audio`,
//! `wiki: formats/graphics` and `wiki: game/text`.

use crate::emi::{Entry, TYPE_CUE, TYPE_LZ77};
use psx_formats::vram;
use psx_formats::{sep, vab};

/// The load-address window that holds CLUT banks
/// (`wiki: formats/graphics#palette`).
const CLUT_BAND: std::ops::RangeInclusive<u32> = 0x8003_3800..=0x8003_6e00;

/// Load addresses that hold code images: the EMI code targets of
/// `config/emi/**/target.toml`, plus the two addresses the asset census's code
/// classifier found without a configured target.
const CODE_ADDRESSES: [u32; 9] = [
    0x8009_6800, // emi/battle/battle/15
    0x800c_1800,
    0x800f_5000, // emi/world00/area030/05, the battle code base
    0x8019_5800, // emi/etc/game/00 (GAME.EMI) and its inline tables
    0x801c_e400,
    0x801d_0c00, // emi/world00/area030/04, emi/battle/battle/03, the etc/ overlays
    0x801e_ec00, // emi/etc/commu00/00
    0x801f_2c00, // the area program code arena
    0x801f_6c00, // the arena's second page
];

/// What this crate can do with a format today.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    /// A decode → encode codec in this crate rebuilds the payload bytes.
    Codec,
    /// The layout is documented; no codec exists here yet.
    Specified,
    /// Neither a spec nor a codec.
    Unknown,
    /// Loaded and executed as code, not an asset format.
    Code,
    /// An all-zero payload: a reserved RAM slot.
    Reserve,
}

impl State {
    /// The state's name in the scoreboard.
    pub fn label(self) -> &'static str {
        match self {
            Self::Codec => "codec",
            Self::Specified => "specified",
            Self::Unknown => "unknown",
            Self::Code => "code",
            Self::Reserve => "reserve",
        }
    }
}

/// One entry format, in scoreboard order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Format {
    VramUpload,
    ClutBank,
    VabHeader,
    VabBody,
    SepPackage,
    Lz77Blob,
    CharacterBlob,
    AreaMessageBank,
    AreaCellGrid,
    AreaScriptBank,
    BattleScriptBank,
    AreaActorDefsTable,
    AreaEncounterTable,
    AreaMapBundle,
    AreaAuxWindow,
    SharedResourceTable,
    CueTable,
    MagicTable,
    CodeImage,
    Reserve,
    Unclassified,
}

impl Format {
    /// Every format, in scoreboard order.
    pub const ALL: [Format; 21] = [
        Format::VramUpload,
        Format::ClutBank,
        Format::VabHeader,
        Format::VabBody,
        Format::SepPackage,
        Format::Lz77Blob,
        Format::CharacterBlob,
        Format::AreaMessageBank,
        Format::AreaCellGrid,
        Format::AreaScriptBank,
        Format::BattleScriptBank,
        Format::AreaActorDefsTable,
        Format::AreaEncounterTable,
        Format::AreaMapBundle,
        Format::AreaAuxWindow,
        Format::SharedResourceTable,
        Format::CueTable,
        Format::MagicTable,
        Format::CodeImage,
        Format::Reserve,
        Format::Unclassified,
    ];

    /// The row name in the scoreboard and the coverage table.
    pub fn name(self) -> &'static str {
        match self {
            Self::VramUpload => "VRAM texture upload (type 3)",
            Self::ClutBank => "CLUT bank (type 0 in the palette arena band)",
            Self::VabHeader => "VAB header (type 6)",
            Self::VabBody => "VAB body (type 7)",
            Self::SepPackage => "SEP sequence package (type 10)",
            Self::Lz77Blob => "character sprite blob, LZ77 (type 1)",
            Self::CharacterBlob => "character sprite blob, raw (type 0)",
            Self::AreaMessageBank => "area message bank (0x80010000)",
            Self::AreaCellGrid => "area packed 4-bit cell grid (0x80032000)",
            Self::AreaScriptBank => "area script bank (0x800D3800)",
            Self::BattleScriptBank => "battle script bank (0x800F0800)",
            Self::AreaActorDefsTable => "area actor defs table (0x800E3800)",
            Self::AreaEncounterTable => "area encounters and enemy records (0x800E4000)",
            Self::AreaMapBundle => "area map bundle (0x80104000)",
            Self::AreaAuxWindow => "area auxiliary module window (0x80117000)",
            Self::SharedResourceTable => "shared resource table (0x8001A000)",
            Self::CueTable => "type-8 cue records (type 8)",
            Self::MagicTable => "magic-effect table (0x800C3800)",
            Self::CodeImage => "code image (configured EMI code targets)",
            Self::Reserve => "reserve (all-zero payload)",
            Self::Unclassified => "unclassified",
        }
    }

    /// What the crate can do with the format today.
    pub fn state(self) -> State {
        match self {
            Self::VramUpload
            | Self::ClutBank
            | Self::VabHeader
            | Self::VabBody
            | Self::SepPackage
            | Self::CueTable
            | Self::AreaActorDefsTable
            | Self::AreaScriptBank
            | Self::AreaMapBundle => State::Codec,
            Self::Lz77Blob
            | Self::CharacterBlob
            | Self::AreaMessageBank
            | Self::AreaCellGrid
            | Self::BattleScriptBank
            | Self::AreaEncounterTable
            | Self::AreaAuxWindow
            | Self::SharedResourceTable => State::Specified,
            Self::MagicTable | Self::Unclassified => State::Unknown,
            Self::CodeImage => State::Code,
            Self::Reserve => State::Reserve,
        }
    }

    /// How well the format itself is documented, for the coverage table.
    pub fn spec_state(self) -> &'static str {
        match self {
            Self::VramUpload
            | Self::ClutBank
            | Self::VabHeader
            | Self::VabBody
            | Self::SepPackage
            | Self::AreaMessageBank
            | Self::AreaCellGrid
            | Self::AreaScriptBank
            | Self::BattleScriptBank
            | Self::AreaActorDefsTable
            | Self::AreaEncounterTable
            | Self::AreaMapBundle
            | Self::SharedResourceTable
            | Self::CueTable => "documented",
            Self::Lz77Blob | Self::CharacterBlob | Self::AreaAuxWindow => "partial",
            Self::MagicTable | Self::Unclassified => "none",
            Self::CodeImage | Self::Reserve => "n/a",
        }
    }

    /// The codec this crate holds for the format, for the coverage table.
    pub fn codec_state(self) -> &'static str {
        match self {
            Self::VramUpload
            | Self::ClutBank
            | Self::VabHeader
            | Self::VabBody
            | Self::SepPackage
            | Self::CueTable
            | Self::AreaActorDefsTable
            | Self::AreaScriptBank
            | Self::AreaMapBundle => "decode+encode",
            Self::Lz77Blob => "decode-only (in the game's own decoder)",
            Self::CodeImage | Self::Reserve => "n/a",
            _ => "none",
        }
    }

    /// The evidence behind the row's numbers.
    pub fn evidence(self) -> &'static str {
        match self {
            Self::VramUpload => "wiki: formats/graphics#chunk-layout; descriptor from the load argument",
            Self::ClutBank => "wiki: formats/graphics#palette; 575 census palettes sit in the band",
            Self::VabHeader => "wiki: formats/audio#bank-layout; paired by load argument",
            Self::VabBody => "wiki: formats/audio#bank-layout; carried by its header",
            Self::SepPackage => "wiki: formats/audio; 13-byte headers tile 119/119",
            Self::Lz77Blob => "src/slus/func_80164064.c decodes 65/65; no encoder in the repo",
            Self::CharacterBlob => "wiki: formats/character#container: raw section offsets and sections",
            Self::AreaMessageBank => "wiki: formats/area (`0x80010000`)",
            Self::AreaCellGrid => "wiki: formats/area (`0x80032000`); reader func_801C6FD4",
            Self::AreaScriptBank => "wiki: formats/area (`0x800D3800`); size-prefixed directory",
            Self::BattleScriptBank => "wiki: formats/area: the 0x800D3800 format, 42 copies",
            Self::AreaActorDefsTable => "wiki: formats/area (`0x800E3800`)",
            Self::AreaEncounterTable => "wiki: formats/area (`0x800E4000`); include/game/enemy.h models it",
            Self::AreaMapBundle => "wiki: formats/area (`0x80104000`)",
            Self::AreaAuxWindow => "wiki: formats/area (`0x80117000`); role open",
            Self::SharedResourceTable => "wiki: game/text#references",
            Self::CueTable => "wiki: formats/audio#emi-type-8-cue-records",
            Self::MagicTable => "no owning doc; 42-3,148 B payloads in BMAGIC*",
            Self::CodeImage => "config/emi/**/target.toml load addresses; disc-census code rows",
            Self::Reserve => "all-zero payload; the loader reserves the destination",
            Self::Unclassified => "no selection rule matched",
        }
    }
}

/// The file extension an entry's stored bytes are written with: the real
/// format's name where the game uses one, and the entry's type tag where the
/// container is the game's own.
///
/// The names are the formats' own: `img` is the game's VRAM upload, `clut`
/// its CLUT bank, `map` its map bundle, `vh`/`vb` the two halves of a VAB and
/// `seq` a SEP package. The rest are the EMI entry with its type tag.
pub fn extension(format: Format) -> &'static str {
    match format {
        Format::VramUpload => "img",
        Format::ClutBank => "clut",
        Format::VabHeader => "vh",
        Format::VabBody => "vb",
        Format::SepPackage => "seq",
        Format::Lz77Blob => "lz77",
        Format::CueTable => "emi8",
        Format::AreaMapBundle => "map",
        _ => "emi0",
    }
}

/// Area message bank (`wiki: formats/area`, `0x80010000`).
const AREA_MESSAGE_BANK: u32 = 0x8001_0000;
/// Shared resource table (`wiki: game/text#references`).
const SHARED_RESOURCE_TABLE: u32 = 0x8001_a000;
/// Packed 4-bit cell grid (`wiki: formats/area`, `0x80032000`).
const AREA_CELL_GRID: u32 = 0x8003_2000;
/// Character bank load arguments: the whole container and the five section
/// positions (`wiki: formats/character#load-arguments`).
const CHARACTER_BLOB_ADDRESSES: [u32; 6] =
    [0x8003_b800, 0x8003_b80c, 0x8005_680c, 0x8005_f80c, 0x8007_180c, 0x8007_a80c];
/// Magic-effect table of the `BMAGIC*` archives; no owning spec.
const MAGIC_TABLE: u32 = 0x800c_3800;
/// Area script bank (`wiki: formats/area`, `0x800D3800`).
const AREA_SCRIPT_BANK: u32 = 0x800d_3800;
/// Scene/step table (`wiki: formats/area`, `0x800E3800`).
const AREA_ACTOR_DEFS_TABLE: u32 = 0x800e_3800;
/// Encounters and enemy records (`wiki: formats/area`, `0x800E4000`).
const AREA_ENCOUNTERS: u32 = 0x800e_4000;
/// Battle script bank: the area script bank format, loaded here.
const BATTLE_SCRIPT_BANK: u32 = 0x800f_0800;
/// Map bundle (`wiki: formats/area`, `0x80104000`).
const AREA_MAP_BUNDLE: u32 = 0x8010_4000;
/// Auxiliary module window (`wiki: formats/area`, `0x80117000`).
const AREA_AUX_WINDOW: u32 = 0x8011_7000;

/// The format one entry belongs to: its file type and load address, with an
/// all-zero payload counted as a reservation wherever it sits.
pub fn classify(entry: &Entry, payload: &[u8]) -> Format {
    if entry.file_type == 0 && payload.iter().all(|byte| *byte == 0) {
        return Format::Reserve;
    }
    match entry.file_type {
        vram::UPLOAD_TYPE => Format::VramUpload,
        vab::HEADER_TYPE => Format::VabHeader,
        vab::BODY_TYPE => Format::VabBody,
        sep::TYPE => Format::SepPackage,
        TYPE_LZ77 => Format::Lz77Blob,
        TYPE_CUE => Format::CueTable,
        0 => classify_data(entry.ram_ptr),
        _ => Format::Unclassified,
    }
}

/// The format of a type-0 payload, by its load address.
fn classify_data(ram_ptr: u32) -> Format {
    if CLUT_BAND.contains(&ram_ptr) {
        return Format::ClutBank;
    }
    if CODE_ADDRESSES.contains(&ram_ptr) {
        return Format::CodeImage;
    }
    match ram_ptr {
        AREA_MESSAGE_BANK => Format::AreaMessageBank,
        SHARED_RESOURCE_TABLE => Format::SharedResourceTable,
        AREA_CELL_GRID => Format::AreaCellGrid,
        a if CHARACTER_BLOB_ADDRESSES.contains(&a) => Format::CharacterBlob,
        MAGIC_TABLE => Format::MagicTable,
        AREA_SCRIPT_BANK => Format::AreaScriptBank,
        AREA_ACTOR_DEFS_TABLE => Format::AreaActorDefsTable,
        AREA_ENCOUNTERS => Format::AreaEncounterTable,
        BATTLE_SCRIPT_BANK => Format::BattleScriptBank,
        AREA_MAP_BUNDLE => Format::AreaMapBundle,
        AREA_AUX_WINDOW => Format::AreaAuxWindow,
        _ => Format::Unclassified,
    }
}
