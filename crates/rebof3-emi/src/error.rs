//! The crate's one error type.

use core::fmt;

use crate::cue;
#[cfg(feature = "std")]
use crate::lz77;

/// Everything a parser, codec or archive operation in this crate can report.
#[derive(Debug)]
pub enum Error {
    /// A file could not be read or written.
    #[cfg(feature = "std")]
    Io(std::io::Error),
    /// A PlayStation format inside an entry is malformed.
    Format(psx_formats::Error),
    /// The container table is malformed.
    InvalidArchive(&'static str),
    EntryOutOfRange {
        index: usize,
        count: usize,
    },
    /// A cue table's size is not a whole number of cue records.
    CueTableSize(usize),
    /// A cue table holds more records than one bank chunk.
    CueTableRecords(usize),
    /// A cue record sets bits the format reserves.
    CueRecord {
        record: usize,
        reason: &'static str,
    },
    /// A type-1 payload is shorter than its header.
    #[cfg(feature = "std")]
    Lz77Truncated,
    /// The type-1 marker word is not `0xFFFF`.
    #[cfg(feature = "std")]
    Lz77BadMarker(u16),
    /// A type-1 control stream ended inside a frame.
    #[cfg(feature = "std")]
    Lz77TruncatedStream,
    /// A type-1 stream decoded to a different size than its header declares.
    #[cfg(feature = "std")]
    Lz77SizeMismatch {
        declared: usize,
        decoded: usize,
    },
    /// A back-reference reaches before the start of the output.
    #[cfg(feature = "std")]
    Lz77DistanceUnderflow {
        distance: usize,
        produced: usize,
    },
    /// A fixed table runs past the end of its payload.
    #[cfg(feature = "std")]
    GameTableShort {
        table: &'static str,
        bytes: usize,
        needed: usize,
    },
    /// A manifest input is unusable: an absolute or parent-relative name, an
    /// entry that is missing or not a regular file, or a name whose resolved
    /// path leaves the input folder. The message names the entry and the file.
    #[cfg(feature = "std")]
    ManifestInput(String),
    /// A game text cannot be encoded: a character or tag the encoding does not have, or an operand that does not fit.
    #[cfg(feature = "std")]
    Text(String),
    /// The characters table cannot be read or written: a count, a name, or the rest of a record that does not fit.
    #[cfg(feature = "std")]
    CharacterTable(String),
    /// A sprite bank's palettes cannot be read or written: a section that is not in the palette layout, or a palette
    /// that does not match the bank it is written to.
    #[cfg(feature = "std")]
    CharacterBank(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(feature = "std")]
            Self::Io(error) => write!(f, "{error}"),
            Self::Format(error) => write!(f, "{error}"),
            Self::InvalidArchive(message) => write!(f, "invalid EMI archive: {message}"),
            Self::EntryOutOfRange { index, count } => {
                write!(f, "entry index {index} is out of range (archive has {count} entries)")
            }
            Self::CueTableSize(bytes) => {
                write!(f, "cue table is {bytes} bytes; a cue record is {} bytes", cue::RECORD_BYTES)
            }
            Self::CueTableRecords(records) => {
                write!(f, "cue table holds {records} records; one bank chunk holds {}", cue::CHUNK_RECORDS)
            }
            Self::CueRecord { record, reason } => write!(f, "cue record {record}: {reason}"),
            #[cfg(feature = "std")]
            Self::Lz77Truncated => write!(f, "payload is shorter than the {}-byte type-1 header", lz77::HEADER),
            #[cfg(feature = "std")]
            Self::Lz77BadMarker(marker) => {
                write!(f, "marker word at offset 4 is {marker:#06x}, not 0xffff")
            }
            #[cfg(feature = "std")]
            Self::Lz77TruncatedStream => f.write_str("stream ended inside a frame"),
            #[cfg(feature = "std")]
            Self::Lz77SizeMismatch { declared, decoded } => {
                write!(f, "header declares {declared} bytes, stream decoded {decoded}")
            }
            #[cfg(feature = "std")]
            Self::Lz77DistanceUnderflow { distance, produced } => {
                write!(f, "distance {distance} needs {produced} produced bytes")
            }
            #[cfg(feature = "std")]
            Self::GameTableShort { table, bytes, needed } => {
                write!(f, "game table {table} needs {needed} bytes; its payload is {bytes}")
            }
            #[cfg(feature = "std")]
            Self::ManifestInput(message) => write!(f, "{message}"),
            #[cfg(feature = "std")]
            Self::Text(message) => write!(f, "{message}"),
            #[cfg(feature = "std")]
            Self::CharacterTable(message) => write!(f, "{message}"),
            #[cfg(feature = "std")]
            Self::CharacterBank(message) => write!(f, "{message}"),
        }
    }
}

#[cfg(feature = "std")]
impl core::error::Error for Error {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Format(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(not(feature = "std"))]
impl core::error::Error for Error {}

#[cfg(feature = "std")]
impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<psx_formats::Error> for Error {
    fn from(error: psx_formats::Error) -> Self {
        Self::Format(error)
    }
}

impl From<::emi::Error> for Error {
    fn from(error: ::emi::Error) -> Self {
        match error {
            #[cfg(feature = "std")]
            ::emi::Error::Io(error) => Self::Io(error),
            ::emi::Error::Format(error) => Self::Format(error),
            ::emi::Error::InvalidArchive(message) => Self::InvalidArchive(message),
            ::emi::Error::EntryOutOfRange { index, count } => Self::EntryOutOfRange { index, count },
            ::emi::Error::CueTableSize(bytes) => Self::CueTableSize(bytes),
            ::emi::Error::CueTableRecords(records) => Self::CueTableRecords(records),
            ::emi::Error::CueRecord { record, reason } => Self::CueRecord { record, reason },
        }
    }
}

/// The crate's result type.
pub type Result<T, E = Error> = core::result::Result<T, E>;
