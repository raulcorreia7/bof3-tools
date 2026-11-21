//! The crate's one error type.

use std::fmt;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Disc(psx_cd::Error),
    Formats(rebof3_formats::Error),
    /// A VAB bank the reader or the SoundFont writer rejects.
    Bank(psx_formats::Error),
    /// A text extraction that needs an extracted disc and has none at this path.
    NoTextDisc(std::path::PathBuf),
    /// A text index row that does not serialize.
    TextIndex(serde_json::Error),
    /// The sound library or a player rejected an item.
    Audio(bof3player::Error),
    /// An archive that has no playable sequence of this id.
    NoSequence {
        path: String,
        sequence: u32,
    },
    /// An `--only` name that is no type.
    UnknownType(String),
    /// An `--audio-format` value that is no format.
    UnknownAudioFormat(String),
    /// A `--mode` value that is neither mode.
    UnknownMode(String),
    /// An `--only` list with no type.
    NoTypes,
    /// A conversion job that does not fit its op.
    Job(String),
    /// A worker thread stopped before its work was sent.
    WorkerStopped,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Disc(error) => write!(f, "{error}"),
            Self::Formats(error) => write!(f, "{error}"),
            Self::Bank(error) => write!(f, "{error}"),
            Self::NoTextDisc(disc) => write!(f, "no extracted disc at {}; pass --text-disc DIR with the tree `bof3 disc extract` wrote", disc.display()),
            Self::TextIndex(error) => write!(f, "{error}"),
            Self::Audio(error) => write!(f, "{error}"),
            Self::NoSequence { path, sequence } => write!(f, "{path} has no playable sequence {sequence}"),
            Self::UnknownType(name) => write!(f, "unknown type {name:?}; the types are {}", crate::types::AssetType::ALL.iter().map(|asset_type| asset_type.name()).collect::<Vec<_>>().join(", ")),
            Self::UnknownAudioFormat(name) => write!(f, "unknown audio format {name:?}; the formats are wav, flac and ogg"),
            Self::UnknownMode(name) => write!(f, "unknown mode {name:?}; the modes are raw and modern"),
            Self::NoTypes => f.write_str("--only needs at least one type"),
            Self::Job(message) => f.write_str(message),
            Self::WorkerStopped => f.write_str("a worker thread stopped early"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Disc(error) => Some(error),
            Self::Formats(error) => Some(error),
            Self::Bank(error) => Some(error),
            Self::TextIndex(error) => Some(error),
            Self::Audio(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<psx_cd::Error> for Error {
    fn from(error: psx_cd::Error) -> Self {
        Self::Disc(error)
    }
}

impl From<psx_formats::Error> for Error {
    fn from(error: psx_formats::Error) -> Self {
        Self::Bank(error)
    }
}

impl From<rebof3_emi::Error> for Error {
    fn from(error: rebof3_emi::Error) -> Self {
        Self::Formats(error.into())
    }
}

impl From<rebof3_emi::ContainerError> for Error {
    fn from(error: rebof3_emi::ContainerError) -> Self {
        rebof3_emi::Error::from(error).into()
    }
}

impl From<rebof3_formats::Error> for Error {
    fn from(error: rebof3_formats::Error) -> Self {
        Self::Formats(error)
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
