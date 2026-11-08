//! The asset kinds `rebof3-assets` sorts the disc into.
//!
//! A kind is a directory of the output tree and a value of the `kind` column of
//! `assets.jsonl`. An EMI entry's kind follows from its
//! [`Format`], so the census in
//! `wiki: formats/conversion#conversion-map` and this table name the
//! same things.

use rebof3_emi::classify::Format;

use crate::error::{Error, Result};

/// What the tool writes for a kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Handling {
    /// The stored bytes and a derived file (PNG, JSON or decoded bytes).
    Decoded,
    /// The stored bytes only: no decoder yet, or the format is a media stream.
    Raw,
    /// Nothing: the payload carries no data.
    Nothing,
}

impl Handling {
    pub fn label(self) -> &'static str {
        match self {
            Self::Decoded => "decoded",
            Self::Raw => "raw",
            Self::Nothing => "none",
        }
    }
}

macro_rules! kinds {
    ($($variant:ident => ($name:literal, $handling:ident, $about:literal)),+ $(,)?) => {
        /// One class of game asset.
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub enum Kind {
            $($variant),+
        }

        impl Kind {
            /// Every kind, in report order.
            pub const ALL: &'static [Kind] = &[$(Kind::$variant),+];

            /// The kind's name on the command line, in `assets.jsonl` and as a directory.
            pub fn name(self) -> &'static str {
                match self { $(Kind::$variant => $name),+ }
            }

            /// What the tool writes for the kind.
            pub fn handling(self) -> Handling {
                match self { $(Kind::$variant => Handling::$handling),+ }
            }

            /// One line on what the kind holds.
            pub fn about(self) -> &'static str {
                match self { $(Kind::$variant => $about),+ }
            }
        }
    };
}

kinds! {
    Executable => ("executable", Raw, "program images: SLUS_004.22 and LOGO.EXE"),
    System => ("system", Raw, "SYSTEM.CNF and the license sectors"),
    Video => ("video", Raw, "MDEC video with XA audio (2336-byte sectors)"),
    XaAudio => ("xa-audio", Raw, "multiplexed XA ADPCM banks (2336-byte sectors)"),
    Cdda => ("cdda", Decoded, "CD-DA tracks: the stored 16-bit stereo PCM and a 44.1 kHz WAV"),
    Texture => ("texture", Decoded, "type-3 VRAM uploads: raw words and a 16bpp PNG view"),
    Palette => ("palette", Decoded, "CLUT banks: raw words, JSON and a swatch PNG"),
    AudioBank => ("audio-bank", Decoded, "VAB header (.vh) and body (.vb) halves, and a SoundFont 2 (.sf2) per bank"),
    AudioSequence => ("audio-sequence", Raw, "SEP sequence packages (.seq)"),
    AudioCues => ("audio-cues", Decoded, "type-8 cue tables: raw records and JSON"),
    CharacterBank => ("character-bank", Decoded, "sprite banks: stored form and, for type 1, the LZ77-decoded blob"),
    AreaMessage => ("area-message", Raw, "area message banks (0x80010000)"),
    AreaCells => ("area-cells", Raw, "packed 4-bit cell grids (0x80032000)"),
    AreaScript => ("area-script", Decoded, "area script banks (0x800D3800): stored bytes and JSON"),
    BattleScript => ("battle-script", Raw, "battle script banks (0x800F0800)"),
    AreaActorDefs => ("area-actor-defs", Decoded, "actor definitions: raw records and JSON"),
    AreaEncounters => ("area-encounters", Decoded, "encounters and enemy records (0x800E4000): stored bytes and JSON"),
    AreaMap => ("area-map", Decoded, "map bundles (0x80104000): stored bytes and the terrain as OBJ, MTL and glTF binary"),
    AreaModule => ("area-module", Raw, "auxiliary module windows (0x80117000)"),
    ResourceTable => ("resource-table", Raw, "shared resource tables (0x8001A000)"),
    MagicTable => ("bmagic-table", Raw, "magic-effect script banks (0x800C3800)"),
    Code => ("code", Raw, "code images loaded from EMI entries"),
    Reserve => ("reserve", Nothing, "all-zero payloads that reserve a RAM slot"),
    Unknown => ("unknown", Raw, "entries and files no rule classifies"),
    DataTable => ("data-table", Decoded, "fixed game tables from ETC/GAME.EMI entry 0: stored bytes and JSON for items, equipment, shops, abilities, level growth"),
    Text => ("text", Decoded, "game strings and message banks as JSON, from the extracted disc"),
}

impl Kind {
    /// The kind of an EMI entry's format.
    pub fn of_format(format: Format) -> Self {
        match format {
            Format::VramUpload => Self::Texture,
            Format::ClutBank => Self::Palette,
            Format::VabHeader | Format::VabBody => Self::AudioBank,
            Format::SepPackage => Self::AudioSequence,
            Format::Lz77Blob | Format::CharacterBlob => Self::CharacterBank,
            Format::AreaMessageBank => Self::AreaMessage,
            Format::AreaCellGrid => Self::AreaCells,
            Format::AreaScriptBank => Self::AreaScript,
            Format::BattleScriptBank => Self::BattleScript,
            Format::AreaActorDefsTable => Self::AreaActorDefs,
            Format::AreaEncounterTable => Self::AreaEncounters,
            Format::AreaMapBundle => Self::AreaMap,
            Format::AreaAuxWindow => Self::AreaModule,
            Format::SharedResourceTable => Self::ResourceTable,
            Format::CueTable => Self::AudioCues,
            Format::MagicTable => Self::MagicTable,
            Format::CodeImage => Self::Code,
            Format::Reserve => Self::Reserve,
            Format::Unclassified => Self::Unknown,
        }
    }

    /// Parses one `--only` name.
    pub fn parse(name: &str) -> Result<Self> {
        Self::ALL.iter().copied().find(|kind| kind.name() == name).ok_or_else(|| {
            let valid: Vec<&str> = Self::ALL.iter().map(|kind| kind.name()).collect();
            Error::Usage(format!("unknown kind {name:?}; the kinds are {}", valid.join(", ")))
        })
    }
}

/// The roles of an output that are the disc's own bytes: the stored payload (`raw`), a VAB bank as header followed
/// by body (`vab`), and a VAG sample as the body stores it (`vag`). Every other role is a conversion.
const ORIGINAL_ROLES: [&str; 3] = ["raw", "vab", "vag"];

/// The container a run writes for rendered audio items.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AudioFormat {
    /// A 16-bit PCM WAVE file.
    #[default]
    Wav,
    /// A lossless FLAC file.
    Flac,
    /// A lossy Ogg Vorbis file.
    Ogg,
}

impl AudioFormat {
    /// Parses an `--audio-format` value.
    pub fn parse(name: &str) -> Result<Self> {
        match name {
            "wav" => Ok(Self::Wav),
            "flac" => Ok(Self::Flac),
            "ogg" => Ok(Self::Ogg),
            other => Err(Error::Usage(format!("unknown audio format {other:?}; the formats are wav, flac and ogg"))),
        }
    }

    /// The file extension, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Wav => "wav",
            Self::Flac => "flac",
            Self::Ogg => "ogg",
        }
    }
}

/// Which form of an asset a run writes.
///
/// `Raw` writes the original bytes as stored on the disc, byte for byte; `Modern` writes the conversion to an open
/// format, which can be lossy; `Both` (the default) writes both. An asset with no conversion has nothing to add in
/// `Modern`, so its original bytes are its export there too.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Mode {
    #[default]
    Both,
    Raw,
    Modern,
}

impl Mode {
    /// Parses a `--mode` value: `raw` or `modern`.
    pub fn parse(name: &str) -> Result<Self> {
        match name {
            "raw" => Ok(Self::Raw),
            "modern" => Ok(Self::Modern),
            other => Err(Error::Usage(format!("unknown mode {other:?}; the modes are raw and modern"))),
        }
    }

    /// Whether an output with this `role` is written in this mode. `converts` says whether the asset it belongs to
    /// has a conversion at all.
    pub fn writes(self, role: &str, converts: bool) -> bool {
        let original = ORIGINAL_ROLES.contains(&role);
        match self {
            Self::Both => true,
            Self::Raw => original,
            Self::Modern => !original || !converts,
        }
    }
}

/// The kinds a run produces, and the form they are written in: all of them, or the `--only` list.
#[derive(Clone, Debug)]
pub struct Selection {
    kinds: Vec<Kind>,
    mode: Mode,
    audio: AudioFormat,
}

impl Selection {
    pub fn all() -> Self {
        Self { kinds: Kind::ALL.to_vec(), mode: Mode::Both, audio: AudioFormat::Wav }
    }

    /// The same selection, written in `mode`.
    pub fn with_mode(mut self, mode: Mode) -> Self {
        self.mode = mode;
        self
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// The same selection, with rendered audio written as `audio`.
    pub fn with_audio(mut self, audio: AudioFormat) -> Self {
        self.audio = audio;
        self
    }

    /// The container rendered audio is written in.
    pub fn audio(&self) -> AudioFormat {
        self.audio
    }

    /// Parses a comma-separated kind list.
    pub fn parse(list: &str) -> Result<Self> {
        let kinds = list
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(Kind::parse)
            .collect::<Result<Vec<_>>>()?;
        if kinds.is_empty() {
            return Err(Error::Usage("--only needs at least one kind".into()));
        }
        Ok(Self { kinds, mode: Mode::Both, audio: AudioFormat::Wav })
    }

    pub fn contains(&self, kind: Kind) -> bool {
        self.kinds.contains(&kind)
    }

    /// Whether the selection leaves out every kind an EMI entry can have.
    pub fn is_only(&self, kind: Kind) -> bool {
        self.kinds == [kind]
    }
}
