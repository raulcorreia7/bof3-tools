//! Status codes, the thread-local error message and the panic boundary.

use std::cell::RefCell;
use std::ffi::{CString, c_char};

/// The result of every fallible C function.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum rebof3_status {
    /// The call succeeded; the thread's last error message is cleared.
    Ok = 0,
    /// A pointer, a length or a value the function cannot use.
    InvalidArgument = 1,
    /// An index names no entry of the table or list.
    InvalidIndex = 2,
    /// The kind, format or direction has no implementation.
    Unsupported = 3,
    /// A file could not be read or written.
    Io = 4,
    /// The input bytes are malformed for the format.
    Format = 5,
    /// The call needs a file, a name or an argument the caller did not supply.
    Usage = 6,
    /// The library panicked; no Rust unwinding crossed the boundary.
    Panic = 7,
}

thread_local! {
    static LAST_ERROR: RefCell<Option<CString>> = const { RefCell::new(None) };
}

/// The last error message of the calling thread, or NULL after a successful call.
///
/// The pointer is NUL-terminated and valid until the next call on the same thread.
#[unsafe(no_mangle)]
pub extern "C" fn rebof3_last_error_message() -> *const c_char {
    LAST_ERROR.with(|last| last.borrow().as_ref().map_or(std::ptr::null(), |message| message.as_ptr()))
}

pub(crate) fn clear_error() {
    LAST_ERROR.with(|last| {
        let _ = last.borrow_mut().take();
    });
}

pub(crate) fn set_error(message: impl std::fmt::Display) {
    let text = message.to_string();
    let message = CString::new(text).unwrap_or_else(|_| CString::new("the error message contained a NUL").expect("a literal has no NUL"));
    LAST_ERROR.with(|last| *last.borrow_mut() = Some(message));
}

/// One failure ready for the C boundary.
#[derive(Debug)]
pub(crate) struct Failure {
    pub(crate) status: rebof3_status,
    pub(crate) message: String,
}

impl Failure {
    pub(crate) fn new(status: rebof3_status, message: impl Into<String>) -> Self {
        Self { status, message: message.into() }
    }
}

pub(crate) fn invalid(message: impl Into<String>) -> Failure {
    Failure::new(rebof3_status::InvalidArgument, message)
}

pub(crate) fn usage(message: impl Into<String>) -> Failure {
    Failure::new(rebof3_status::Usage, message)
}

pub(crate) fn unsupported(message: impl Into<String>) -> Failure {
    Failure::new(rebof3_status::Unsupported, message)
}

/// Runs `f` with the thread's error cleared, catches a panic and returns the status.
pub(crate) fn guarded<T>(f: impl FnOnce() -> Result<T, Failure>) -> Result<T, rebof3_status> {
    clear_error();
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(failure)) => {
            set_error(&failure.message);
            Err(failure.status)
        }
        Err(_payload) => {
            set_error("the library panicked");
            Err(rebof3_status::Panic)
        }
    }
}

impl From<std::io::Error> for Failure {
    fn from(error: std::io::Error) -> Self {
        Self { status: rebof3_status::Io, message: error.to_string() }
    }
}

fn has_io_source(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut current = Some(error);
    while let Some(error) = current {
        if error.downcast_ref::<std::io::Error>().is_some() {
            return true;
        }
        current = error.source();
    }
    false
}

impl From<rebof3_assets::Error> for Failure {
    fn from(error: rebof3_assets::Error) -> Self {
        match error {
            rebof3_assets::Error::Io(inner) => Self { status: rebof3_status::Io, message: inner.to_string() },
            rebof3_assets::Error::Disc(inner) => {
                let status = if has_io_source(&inner) { rebof3_status::Io } else { rebof3_status::Format };
                Self { status, message: inner.to_string() }
            }
            rebof3_assets::Error::Formats(inner) => Self::from(inner),
            rebof3_assets::Error::Usage(message) | rebof3_assets::Error::Text(message) => Self { status: rebof3_status::Usage, message },
            other => Self { status: rebof3_status::Format, message: other.to_string() },
        }
    }
}

impl From<rebof3_formats::Error> for Failure {
    fn from(error: rebof3_formats::Error) -> Self {
        match error {
            rebof3_formats::Error::Container(inner) => Self::from(inner),
            other => Self { status: rebof3_status::Format, message: other.to_string() },
        }
    }
}

impl From<rebof3_emi::Error> for Failure {
    fn from(error: rebof3_emi::Error) -> Self {
        let status = match &error {
            rebof3_emi::Error::Io(_) => rebof3_status::Io,
            rebof3_emi::Error::EntryOutOfRange { .. } => rebof3_status::InvalidIndex,
            rebof3_emi::Error::ManifestInput(_) => rebof3_status::Usage,
            _ => rebof3_status::Format,
        };
        Self { status, message: error.to_string() }
    }
}
