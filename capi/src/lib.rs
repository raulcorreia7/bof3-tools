//! The C interface to the Breath of Fire III libraries.
//!
//! This crate holds every `extern "C"` function, the status codes, the thread-local error and the
//! ownership rules documented in `docs/c-api.md`. The library crates stay pure Rust. Every function
//! that can fail returns [`rebof3_status`]; a returned buffer or string is owned by the caller and
//! freed with the matching `_free`; a panic is caught at the boundary and reported as
//! [`rebof3_status::Panic`].

// Every exported type is named for C, which is not Rust's CamelCase.
#![allow(non_camel_case_types)]

mod emi;
mod error;

pub use error::rebof3_status;

use std::ffi::c_char;

/// The ABI version of this header. It changes only when an existing signature or layout changes.
#[unsafe(no_mangle)]
pub extern "C" fn rebof3_abi_version() -> u32 {
    1
}

/// A byte buffer owned by the caller until `rebof3_buffer_free`.
#[repr(C)]
#[derive(Debug)]
pub struct rebof3_buffer {
    pub data: *mut u8,
    pub len: usize,
}

/// A UTF-8 string owned by the caller until `rebof3_string_free`.
#[repr(C)]
#[derive(Debug)]
pub struct rebof3_string {
    pub data: *mut c_char,
    pub len: usize,
}

impl rebof3_buffer {
    pub(crate) fn from_vec(bytes: Vec<u8>) -> Self {
        let bytes = bytes.into_boxed_slice();
        let len = bytes.len();
        Self { data: Box::into_raw(bytes) as *mut u8, len }
    }
}

impl rebof3_string {
    pub(crate) fn from_string(text: String) -> Self {
        let bytes = text.into_bytes().into_boxed_slice();
        let len = bytes.len();
        Self { data: Box::into_raw(bytes) as *mut c_char, len }
    }
}

unsafe fn free_boxed(data: *mut u8, len: usize) {
    if !data.is_null() {
        unsafe { drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(data, len))) };
    }
}

/// Releases a buffer returned by this library and zeroes its fields.
///
/// # Safety
/// `buffer` must be NULL or a buffer this library returned, not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rebof3_buffer_free(buffer: *mut rebof3_buffer) {
    if buffer.is_null() {
        return;
    }
    let buffer = unsafe { &mut *buffer };
    unsafe { free_boxed(buffer.data, buffer.len) };
    buffer.data = std::ptr::null_mut();
    buffer.len = 0;
}

/// Releases a string returned by this library and zeroes its fields.
///
/// # Safety
/// `string` must be NULL or a string this library returned, not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rebof3_string_free(string: *mut rebof3_string) {
    if string.is_null() {
        return;
    }
    let string = unsafe { &mut *string };
    unsafe { free_boxed(string.data.cast::<u8>(), string.len) };
    string.data = std::ptr::null_mut();
    string.len = 0;
}

/// Forces this crate's object code into a test binary that calls the C smoke test.
#[doc(hidden)]
pub fn smoke_anchor() -> u32 {
    rebof3_abi_version()
}
