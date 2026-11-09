//! The crate's one error type.

use std::fmt;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Disc(psx_cd::Error),
    Formats(rebof3_formats::Error),
    /// A VAB bank the reader or the SoundFont writer rejects.
    Bank(psx_formats::Error),
    /// A command line the tool cannot act on.
    Usage(String),
    /// A text extraction that could not run.
    Text(String),
    /// A sound item that could not be read or rendered.
    Audio(String),
    /// An open-format file that an import cannot read back to the stored bytes.
    Import(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Disc(error) => write!(f, "{error}"),
            Self::Formats(error) => write!(f, "{error}"),
            Self::Bank(error) => write!(f, "{error}"),
            Self::Usage(message) | Self::Text(message) | Self::Audio(message) | Self::Import(message) => {
                f.write_str(message)
            }
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
            Self::Usage(_) | Self::Text(_) | Self::Audio(_) | Self::Import(_) => None,
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

impl From<rebof3_formats::Error> for Error {
    fn from(error: rebof3_formats::Error) -> Self {
        Self::Formats(error)
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
