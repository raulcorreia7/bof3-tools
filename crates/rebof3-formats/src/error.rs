//! The error type of the format modules.

use std::fmt;

/// Everything a format module can report when it decodes, encodes, exports or imports.
#[derive(Debug)]
pub enum Error {
    /// The container, the primitives or a PlayStation format below the module failed.
    Container(rebof3_emi::Error),
    /// A scene table is not a count followed by that many 8-byte records.
    SceneTableShape {
        /// The payload size.
        bytes: usize,
        /// The record count it declares.
        count: u32,
    },
    /// A map bundle payload is shorter than its own header describes.
    MapBundleTooShort(usize),
    /// A map bundle's sections do not satisfy the identities its readers assume.
    MapBundleShape(&'static str),
    /// A script bank's directory or a section's table words do not parse as the `0x800D3800`
    /// layout (`wiki: formats/area`).
    ScriptBankShape(&'static str),
    /// An open-format file that an import cannot read back to the stored bytes.
    Import(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Container(error) => write!(f, "{error}"),
            Self::SceneTableShape { bytes, count } => write!(
                f,
                "scene table is {bytes} bytes declaring {count} records; the shapes are a count \
                 of at most {} read as one byte and 4 + 8*count bytes",
                crate::scene::MAX_RECORDS
            ),
            Self::MapBundleTooShort(bytes) => {
                write!(f, "map bundle is {bytes} bytes; its header and sections need more")
            }
            Self::MapBundleShape(reason) => write!(f, "map bundle: {reason}"),
            Self::ScriptBankShape(reason) => write!(f, "script bank: {reason}"),
            Self::Import(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Container(error) => Some(error),
            _ => None,
        }
    }
}

impl From<rebof3_emi::Error> for Error {
    fn from(error: rebof3_emi::Error) -> Self {
        Self::Container(error)
    }
}

impl From<psx_formats::Error> for Error {
    fn from(error: psx_formats::Error) -> Self {
        Self::Container(error.into())
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Container(error.into())
    }
}

/// The result type of the format modules.
pub type Result<T, E = Error> = std::result::Result<T, E>;
