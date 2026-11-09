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
    /// A fixed game table runs past the end of its payload.
    GameTableShort {
        /// The table's name.
        table: &'static str,
        /// The payload size.
        bytes: usize,
        /// The size the table needs.
        needed: usize,
    },
    /// An encounter block is shorter than its 1,160 bytes.
    EncounterBlockShort {
        /// The block size.
        bytes: usize,
    },
    /// The characters table cannot be read or written: a count, a name, or the rest of a record that does not fit.
    CharacterTable(String),
    /// A sprite bank's palettes cannot be read or written: a section that is not in the palette layout, or a palette
    /// that does not match the bank it is written to.
    CharacterBank(String),
    /// An animation stream does not read or write in the layout `Actor_SeekScript` and the step
    /// functions assume.
    AnimationStream(String),
    /// A sprite bank's sections, frame sheets or animations do not read or write in the layout the field
    /// reader assumes.
    SpriteBank(String),
    /// An encounter formation that does not place: a missing encounter, enemy or placement table.
    Formation(String),
    /// A sound item that could not be read or rendered.
    Audio(String),
    /// An open-format file that an import cannot read back to the stored bytes.
    Import(String),
    /// A field table of the executable or a setup stream of an area program that does not read.
    Field(String),
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
            Self::EncounterBlockShort { bytes } => {
                write!(f, "encounter block is {bytes} bytes; it needs 1160")
            }
            Self::GameTableShort { table, bytes, needed } => {
                write!(f, "game table {table} needs {needed} bytes; its payload is {bytes}")
            }
            Self::CharacterTable(message) | Self::CharacterBank(message) => f.write_str(message),
            Self::AnimationStream(message) => write!(f, "animation stream: {message}"),
            Self::SpriteBank(message) => write!(f, "sprite bank: {message}"),
            Self::Formation(message) => write!(f, "formation: {message}"),
            Self::Audio(message) | Self::Import(message) => f.write_str(message),
            Self::Field(message) => write!(f, "field: {message}"),
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
