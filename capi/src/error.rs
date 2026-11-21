//! The mappings from the library crates' errors to a failure, over the shared `psx-capi` guard.

use psx_capi::{Fail, psx_status};

/// A failure the library crates' errors convert into with `?`.
#[derive(Debug)]
pub(crate) struct Failure(Fail);

impl From<Fail> for Failure {
    fn from(fail: Fail) -> Self {
        Self(fail)
    }
}

impl From<std::io::Error> for Failure {
    fn from(error: std::io::Error) -> Self {
        Self(error.into())
    }
}

fn has_io_source(error: &(dyn std::error::Error + 'static)) -> bool {
    std::iter::successors(Some(error), |error| error.source()).any(|error| error.is::<std::io::Error>())
}

impl From<rebof3_assets::Error> for Failure {
    fn from(error: rebof3_assets::Error) -> Self {
        match error {
            rebof3_assets::Error::Io(inner) => inner.into(),
            rebof3_assets::Error::Disc(inner) => {
                let status = if has_io_source(&inner) { psx_status::Io } else { psx_status::Format };
                Self(Fail::new(status, inner.to_string()))
            }
            rebof3_assets::Error::Formats(inner) => inner.into(),
            rebof3_assets::Error::Job(message) => invalid(message),
            rebof3_assets::Error::UnknownType(_)
            | rebof3_assets::Error::UnknownAudioFormat(_)
            | rebof3_assets::Error::UnknownMode(_)
            | rebof3_assets::Error::NoTypes
            | rebof3_assets::Error::NoTextDisc(_) => usage(error.to_string()),
            other => Self(Fail::new(psx_status::Format, other.to_string())),
        }
    }
}

impl From<rebof3_formats::Error> for Failure {
    fn from(error: rebof3_formats::Error) -> Self {
        match error {
            rebof3_formats::Error::Container(inner) => inner.into(),
            other => Self(Fail::new(psx_status::Format, other.to_string())),
        }
    }
}

impl From<rebof3_emi::Error> for Failure {
    fn from(error: rebof3_emi::Error) -> Self {
        let status = match &error {
            rebof3_emi::Error::Io(_) => psx_status::Io,
            rebof3_emi::Error::EntryOutOfRange { .. } => psx_status::InvalidIndex,
            rebof3_emi::Error::ManifestInput(_) => psx_status::Usage,
            _ => psx_status::Format,
        };
        Self(Fail::new(status, error.to_string()))
    }
}

pub(crate) fn invalid(message: impl Into<String>) -> Failure {
    Failure(Fail::invalid(message))
}

pub(crate) fn usage(message: impl Into<String>) -> Failure {
    Failure(Fail::usage(message))
}

pub(crate) fn unsupported(message: impl Into<String>) -> Failure {
    Failure(Fail::unsupported(message))
}

/// Runs `body` under the shared panic guard and returns its status.
pub(crate) fn guarded(body: impl FnOnce() -> Result<(), Failure>) -> psx_status {
    psx_capi::guard(|| body().map_err(|failure| failure.0))
}
