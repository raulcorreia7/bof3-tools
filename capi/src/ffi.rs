//! Pointer checks and slice conversions shared by the exported modules.

use std::ffi::c_char;

use crate::error::{Failure, invalid};

pub(crate) unsafe fn out_ref<'a, T>(pointer: *mut T, what: &str) -> Result<&'a mut T, Failure> {
    if pointer.is_null() {
        return Err(invalid(format!("{what} is NULL")));
    }
    Ok(unsafe { &mut *pointer })
}

pub(crate) unsafe fn input<'a>(data: *const u8, len: usize, what: &str) -> Result<&'a [u8], Failure> {
    if data.is_null() {
        if len == 0 {
            return Ok(&[]);
        }
        return Err(invalid(format!("{what} is NULL with a nonzero length")));
    }
    Ok(unsafe { std::slice::from_raw_parts(data, len) })
}

pub(crate) unsafe fn array<'a, T>(data: *const T, count: usize, what: &str) -> Result<&'a [T], Failure> {
    if count == 0 {
        return Ok(&[]);
    }
    if data.is_null() {
        return Err(invalid(format!("{what} is NULL with a nonzero count")));
    }
    Ok(unsafe { std::slice::from_raw_parts(data, count) })
}

pub(crate) unsafe fn utf8<'a>(data: *const c_char, len: usize, what: &str) -> Result<&'a str, Failure> {
    let bytes = unsafe { input(data.cast::<u8>(), len, what)? };
    std::str::from_utf8(bytes).map_err(|_| invalid(format!("{what} is not UTF-8")))
}

pub(crate) unsafe fn optional_utf8<'a>(data: *const c_char, len: usize, what: &str) -> Result<Option<&'a str>, Failure> {
    if data.is_null() {
        if len == 0 {
            return Ok(None);
        }
        return Err(invalid(format!("{what} is NULL with a nonzero length")));
    }
    Ok(Some(unsafe { utf8(data, len, what)? }))
}

pub(crate) unsafe fn optional_out<'a, T>(pointer: *mut T) -> Option<&'a mut T> {
    if pointer.is_null() { None } else { Some(unsafe { &mut *pointer }) }
}
