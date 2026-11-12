//! The EMI container functions.

use std::ffi::c_char;
use std::path::Path;

use rebof3_emi_lib::Archive;
use rebof3_emi_lib::emi::Entry;
use rebof3_emi_lib::pack::{self, Payload};

use ::rebof3_emi as rebof3_emi_lib;

use crate::error::{Failure, guarded, invalid, rebof3_status, usage};
use crate::{rebof3_buffer, rebof3_string};

/// One EMI table entry, as the C caller sees it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct rebof3_emi_entry {
    pub size: u32,
    pub ram_ptr: u32,
    pub first4: u32,
    pub file_type: u16,
    pub reserved: u16,
    pub offset: u64,
}

/// One replacement for `rebof3_emi_rebuild`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct rebof3_emi_replacement {
    pub index: usize,
    pub data: *const u8,
    pub len: usize,
}

/// One payload for `rebof3_emi_pack_payloads`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct rebof3_emi_payload {
    pub data: *const u8,
    pub len: usize,
    pub file_type: u16,
    pub reserved: u16,
    pub ram_ptr: u32,
}

/// An open EMI table and whether it has a source file.
pub struct rebof3_emi {
    pub(crate) archive: Archive,
    pub(crate) from_path: bool,
}

unsafe fn archive_ref<'a>(archive: *const rebof3_emi) -> Result<&'a rebof3_emi, Failure> {
    unsafe { archive.as_ref() }.ok_or_else(|| invalid("the archive pointer is NULL"))
}

unsafe fn out_ref<'a, T>(pointer: *mut T, what: &str) -> Result<&'a mut T, Failure> {
    if pointer.is_null() {
        return Err(invalid(format!("{what} is NULL")));
    }
    Ok(unsafe { &mut *pointer })
}

unsafe fn input<'a>(data: *const u8, len: usize, what: &str) -> Result<&'a [u8], Failure> {
    if data.is_null() {
        if len == 0 {
            return Ok(&[]);
        }
        return Err(invalid(format!("{what} is NULL with a nonzero length")));
    }
    Ok(unsafe { std::slice::from_raw_parts(data, len) })
}

unsafe fn array<'a, T>(data: *const T, count: usize, what: &str) -> Result<&'a [T], Failure> {
    if count == 0 {
        return Ok(&[]);
    }
    if data.is_null() {
        return Err(invalid(format!("{what} is NULL with a nonzero count")));
    }
    Ok(unsafe { std::slice::from_raw_parts(data, count) })
}

unsafe fn utf8<'a>(data: *const c_char, len: usize, what: &str) -> Result<&'a str, Failure> {
    let bytes = unsafe { input(data.cast::<u8>(), len, what)? };
    std::str::from_utf8(bytes).map_err(|_| invalid(format!("{what} is not UTF-8")))
}

fn entry_view(entry: &Entry) -> rebof3_emi_entry {
    rebof3_emi_entry { size: entry.size, ram_ptr: entry.ram_ptr, first4: entry.first4, file_type: entry.file_type, reserved: 0, offset: entry.offset }
}

fn index_failure(index: usize, count: usize) -> Failure {
    Failure::new(rebof3_status::InvalidIndex, format!("entry index {index} is out of range (archive has {count} entries)"))
}

/// Opens the EMI container at `path`.
///
/// # Safety
/// `path` must point to `path_len` readable bytes and `out_archive` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rebof3_emi_open_path(path: *const c_char, path_len: usize, out_archive: *mut *mut rebof3_emi) -> rebof3_status {
    match guarded(|| {
        let path = unsafe { utf8(path, path_len, "path")? };
        let out_archive = unsafe { out_ref(out_archive, "out_archive")? };
        let archive = Archive::open(Path::new(path))?;
        *out_archive = Box::into_raw(Box::new(rebof3_emi { archive, from_path: true }));
        Ok(())
    }) {
        Ok(()) => rebof3_status::Ok,
        Err(status) => status,
    }
}

/// Parses the table of the container in `bytes`; the caller keeps `bytes` alive for payload reads.
///
/// # Safety
/// `bytes` must point to `bytes_len` readable bytes and `out_archive` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rebof3_emi_parse(bytes: *const u8, bytes_len: usize, out_archive: *mut *mut rebof3_emi) -> rebof3_status {
    match guarded(|| {
        let bytes = unsafe { input(bytes, bytes_len, "bytes")? };
        let out_archive = unsafe { out_ref(out_archive, "out_archive")? };
        let archive = Archive::parse(bytes)?;
        *out_archive = Box::into_raw(Box::new(rebof3_emi { archive, from_path: false }));
        Ok(())
    }) {
        Ok(()) => rebof3_status::Ok,
        Err(status) => status,
    }
}

/// Releases an archive handle.
///
/// # Safety
/// `archive` must be NULL or a handle this library returned, not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rebof3_emi_free(archive: *mut rebof3_emi) {
    if archive.is_null() {
        return;
    }
    unsafe { drop(Box::from_raw(archive)) };
}

/// Reads the table's version word.
///
/// # Safety
/// `archive` must be a live handle and `out_version` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rebof3_emi_version(archive: *const rebof3_emi, out_version: *mut u32) -> rebof3_status {
    match guarded(|| {
        let archive = unsafe { archive_ref(archive)? };
        let out_version = unsafe { out_ref(out_version, "out_version")? };
        *out_version = archive.archive.version();
        Ok(())
    }) {
        Ok(()) => rebof3_status::Ok,
        Err(status) => status,
    }
}

/// Reads the number of table entries.
///
/// # Safety
/// `archive` must be a live handle and `out_count` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rebof3_emi_entry_count(archive: *const rebof3_emi, out_count: *mut usize) -> rebof3_status {
    match guarded(|| {
        let archive = unsafe { archive_ref(archive)? };
        let out_count = unsafe { out_ref(out_count, "out_count")? };
        *out_count = archive.archive.entries().len();
        Ok(())
    }) {
        Ok(()) => rebof3_status::Ok,
        Err(status) => status,
    }
}

/// Reads one table entry.
///
/// # Safety
/// `archive` must be a live handle and `out_entry` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rebof3_emi_entry_at(archive: *const rebof3_emi, index: usize, out_entry: *mut rebof3_emi_entry) -> rebof3_status {
    match guarded(|| {
        let archive = unsafe { archive_ref(archive)? };
        let out_entry = unsafe { out_ref(out_entry, "out_entry")? };
        let entry = archive.archive.entries().get(index).ok_or_else(|| index_failure(index, archive.archive.entries().len()))?;
        *out_entry = entry_view(entry);
        Ok(())
    }) {
        Ok(()) => rebof3_status::Ok,
        Err(status) => status,
    }
}

/// Borrows the payload of one entry from `container`, the bytes the table was parsed from.
///
/// # Safety
/// `archive` must be a live handle, `container` must point to `container_len` readable bytes and
/// the out pointers must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rebof3_emi_payload_at(archive: *const rebof3_emi, container: *const u8, container_len: usize, index: usize, out_data: *mut *const u8, out_len: *mut usize) -> rebof3_status {
    match guarded(|| {
        let archive = unsafe { archive_ref(archive)? };
        let container = unsafe { input(container, container_len, "container")? };
        let out_data = unsafe { out_ref(out_data, "out_data")? };
        let out_len = unsafe { out_ref(out_len, "out_len")? };
        let payload = archive.archive.payload(container, index)?;
        *out_data = payload.as_ptr();
        *out_len = payload.len();
        Ok(())
    }) {
        Ok(()) => rebof3_status::Ok,
        Err(status) => status,
    }
}

/// Reads one entry of the container at `path` into an owned buffer.
///
/// # Safety
/// `path` must point to `path_len` readable bytes and `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rebof3_emi_read_entry(path: *const c_char, path_len: usize, index: usize, out: *mut rebof3_buffer) -> rebof3_status {
    match guarded(|| {
        let path = unsafe { utf8(path, path_len, "path")? };
        let out = unsafe { out_ref(out, "out")? };
        let bytes = rebof3_emi_lib::read_entry(Path::new(path), index)?;
        *out = rebof3_buffer::from_vec(bytes);
        Ok(())
    }) {
        Ok(()) => rebof3_status::Ok,
        Err(status) => status,
    }
}

/// Writes one entry of a path-opened archive to `path`.
///
/// # Safety
/// `archive` must be a path-opened handle and `path` must point to `path_len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rebof3_emi_extract_entry(archive: *const rebof3_emi, index: usize, path: *const c_char, path_len: usize) -> rebof3_status {
    match guarded(|| {
        let archive = unsafe { archive_ref(archive)? };
        if !archive.from_path {
            return Err(usage("the archive was parsed from memory; extraction needs an archive opened from a path"));
        }
        let path = unsafe { utf8(path, path_len, "path")? };
        archive.archive.extract_entry(index, Path::new(path))?;
        Ok(())
    }) {
        Ok(()) => rebof3_status::Ok,
        Err(status) => status,
    }
}

/// Writes every entry of a path-opened archive into `directory`.
///
/// # Safety
/// `archive` must be a path-opened handle and `directory` must point to `directory_len` readable
/// bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rebof3_emi_extract_all(archive: *const rebof3_emi, directory: *const c_char, directory_len: usize, typed: u32) -> rebof3_status {
    match guarded(|| {
        let archive = unsafe { archive_ref(archive)? };
        if !archive.from_path {
            return Err(usage("the archive was parsed from memory; extraction needs an archive opened from a path"));
        }
        let directory = unsafe { utf8(directory, directory_len, "directory")? };
        archive.archive.extract_all(Path::new(directory), typed != 0)?;
        Ok(())
    }) {
        Ok(()) => rebof3_status::Ok,
        Err(status) => status,
    }
}

/// Writes the container's `emi.json` text.
///
/// # Safety
/// `archive` must be a live handle and `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rebof3_emi_manifest_json(archive: *const rebof3_emi, typed: u32, out: *mut rebof3_string) -> rebof3_status {
    match guarded(|| {
        let archive = unsafe { archive_ref(archive)? };
        let out = unsafe { out_ref(out, "out")? };
        *out = rebof3_string::from_string(archive.archive.manifest_json(typed != 0));
        Ok(())
    }) {
        Ok(()) => rebof3_status::Ok,
        Err(status) => status,
    }
}

/// Rebuilds a container from its entries, replacing the payloads named in `replacements`.
///
/// # Safety
/// `container` must point to `container_len` readable bytes, each replacement must point to its
/// `len` readable bytes, and `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rebof3_emi_rebuild(
    container: *const u8,
    container_len: usize,
    replacements: *const rebof3_emi_replacement,
    replacement_count: usize,
    out: *mut rebof3_buffer,
) -> rebof3_status {
    match guarded(|| {
        let container = unsafe { input(container, container_len, "container")? };
        let replacements = unsafe { array(replacements, replacement_count, "replacements")? };
        let out = unsafe { out_ref(out, "out")? };
        let mut pairs = Vec::with_capacity(replacements.len());
        for replacement in replacements {
            let bytes = unsafe { input(replacement.data, replacement.len, "replacement data")? };
            pairs.push((replacement.index, bytes));
        }
        let rebuilt = pack::rebuild(container, &pairs)?;
        *out = rebof3_buffer::from_vec(rebuilt);
        Ok(())
    }) {
        Ok(()) => rebof3_status::Ok,
        Err(status) => status,
    }
}

/// Packs payloads into a new container in memory.
///
/// # Safety
/// Each payload must point to its `len` readable bytes and `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rebof3_emi_pack_payloads(archive_version: u32, payloads: *const rebof3_emi_payload, payload_count: usize, out: *mut rebof3_buffer) -> rebof3_status {
    match guarded(|| {
        let payloads = unsafe { array(payloads, payload_count, "payloads")? };
        let out = unsafe { out_ref(out, "out")? };
        let mut inputs = Vec::with_capacity(payloads.len());
        for payload in payloads {
            let bytes = unsafe { input(payload.data, payload.len, "payload data")? };
            inputs.push(Payload { bytes, file_type: payload.file_type, ram_ptr: payload.ram_ptr });
        }
        let packed = pack::pack_payloads(archive_version, &inputs)?;
        *out = rebof3_buffer::from_vec(packed);
        Ok(())
    }) {
        Ok(()) => rebof3_status::Ok,
        Err(status) => status,
    }
}
