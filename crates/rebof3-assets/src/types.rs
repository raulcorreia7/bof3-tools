//! The asset types `rebof3-assets` sorts the disc into.
//!
//! A type is a directory of the output tree and a value of the `asset_type` column of
//! `assets.jsonl`. An EMI entry's type follows from its
//! [`Format`], so the census in
//! `wiki: formats/conversion#conversion-map` and this table name the
//! same things.

use rebof3_formats::Format;

use crate::error::{Error, Result};

/// What the tool writes for a type.
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

macro_rules! types {
    ($($variant:ident => ($name:literal, $handling:ident, $about:literal)),+ $(,)?) => {
        /// One class of game asset.
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub enum AssetType {
            $($variant),+
        }

        impl AssetType {
            /// Every type, in report order.
            pub const ALL: &'static [AssetType] = &[$(AssetType::$variant),+];

            /// The type's name on the command line, in `assets.jsonl` and as a directory.
            pub fn name(self) -> &'static str {
                match self { $(AssetType::$variant => $name),+ }
            }

            /// What the tool writes for the type.
            pub fn handling(self) -> Handling {
                match self { $(AssetType::$variant => Handling::$handling),+ }
            }

            /// One line on what the type holds.
            pub fn about(self) -> &'static str {
                match self { $(AssetType::$variant => $about),+ }
            }
        }
    };
}

types! {
    Executable => ("executable", Raw, "program images: SLUS_004.22 and LOGO.EXE"),
    System => ("system", Raw, "SYSTEM.CNF and the license sectors"),
    Video => ("video", Raw, "MDEC video with XA audio (2336-byte sectors)"),
    XaAudio => ("xa-audio", Raw, "multiplexed XA ADPCM banks (2336-byte sectors)"),
    Cdda => ("cdda", Decoded, "CD-DA tracks: the stored 16-bit stereo PCM and a 44.1 kHz WAV"),
    Texture => ("texture", Decoded, "type-3 VRAM uploads: raw words and a 16bpp PNG view"),
    Palette => ("palette", Decoded, "CLUT blocks: raw words, JSON and a swatch PNG"),
    AudioBank => ("audio-bank", Decoded, "VAB header (.vh) and body (.vb) halves, and a SoundFont 2 (.sf2) per bank"),
    AudioSequence => ("audio-sequence", Raw, "SEP sequence packages (.seq)"),
    AudioCues => ("audio-cues", Decoded, "type-8 cue tables: raw records and JSON"),
    CharacterSprites => ("character-sprites", Decoded, "character sprites: stored form and, for type 1, the LZ77-decoded blob"),
    AreaMessage => ("area-message", Raw, "area message tables (0x80010000)"),
    AreaCells => ("area-cells", Raw, "packed 4-bit cell grids (0x80032000)"),
    AreaScript => ("area-script", Decoded, "area script tables (0x800D3800): stored bytes, JSON and the ops as text"),
    BattleScript => ("battle-script", Decoded, "battle script tables (0x800F0800): stored bytes, JSON and the ops as text"),
    AreaActorDefs => ("area-actor-defs", Decoded, "actor definitions: raw records and JSON"),
    AreaEncounters => ("area-encounters", Decoded, "encounters and enemy records (0x800E4000): stored bytes and JSON"),
    AreaMap => ("area-map", Decoded, "map bundles (0x80104000): stored bytes and the terrain as OBJ, MTL and glTF binary"),
    AreaModule => ("area-module", Raw, "auxiliary module windows (0x80117000)"),
    ResourceTable => ("resource-table", Raw, "shared resource tables (0x8001A000)"),
    MagicScriptTable => ("bmagic-script", Decoded, "magic-effect script tables (0x800C3800): stored bytes, JSON and the ops as text"),
    Code => ("code", Raw, "code images loaded from EMI entries"),
    Reserve => ("reserve", Nothing, "all-zero payloads that reserve a RAM slot"),
    Unknown => ("unknown", Raw, "entries and files no rule classifies"),
    DataTable => ("data-table", Decoded, "fixed game tables from ETC/GAME.EMI entry 0: stored bytes and JSON for items, equipment, shops, abilities, level growth"),
    Text => ("text", Decoded, "game strings and message tables as JSON, from the extracted disc"),
}

impl AssetType {
    /// The type of an EMI entry's format, from the format's registry row.
    pub fn of_format(format: Format) -> Self {
        let name = format.describe().asset_type;
        Self::ALL.iter().copied().find(|asset_type| asset_type.name() == name).unwrap_or_else(|| panic!("{name} is not a type"))
    }

    /// The current name of a type named by an earlier release; any other name comes back unchanged.
    pub fn canonical(name: &str) -> &str {
        match name {
            "character-bank" => "character-sprites",
            "bmagic-table" => "bmagic-script",
            other => other,
        }
    }

    /// Parses one `--only` name.
    pub fn parse(name: &str) -> Result<Self> {
        let name = Self::canonical(name);
        Self::ALL.iter().copied().find(|asset_type| asset_type.name() == name).ok_or_else(|| Error::UnknownType(name.to_string()))
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
            other => Err(Error::UnknownAudioFormat(other.to_string())),
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
            other => Err(Error::UnknownMode(other.to_string())),
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

/// The types a run produces, and the form they are written in: all of them, or the `--only` list.
#[derive(Clone, Debug)]
pub struct Selection {
    types: Vec<AssetType>,
    mode: Mode,
    audio: AudioFormat,
}

impl Selection {
    pub fn all() -> Self {
        Self { types: AssetType::ALL.to_vec(), mode: Mode::Both, audio: AudioFormat::Wav }
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

    /// Parses a comma-separated type list.
    pub fn parse(list: &str) -> Result<Self> {
        let types = list.split(',').map(str::trim).filter(|name| !name.is_empty()).map(AssetType::parse).collect::<Result<Vec<_>>>()?;
        if types.is_empty() {
            return Err(Error::NoTypes);
        }
        Ok(Self { types, mode: Mode::Both, audio: AudioFormat::Wav })
    }

    pub fn contains(&self, asset_type: AssetType) -> bool {
        self.types.contains(&asset_type)
    }

    /// Whether the selection leaves out every type an EMI entry can have.
    pub fn is_only(&self, asset_type: AssetType) -> bool {
        self.types == [asset_type]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_format_names_a_type() {
        for format in Format::ALL {
            assert!(AssetType::ALL.iter().any(|asset_type| asset_type.name() == format.describe().asset_type), "{}", format.name());
        }
    }
}
