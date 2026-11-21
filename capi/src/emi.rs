//! The EMI container functions beyond the shared archive handle: extraction, manifest, repack.

use std::ffi::c_char;

use psx_capi::{array, handle_ref, input, map_emi, out_ref, owned_buffer, owned_string, path_arg, psx_buffer, psx_emi, psx_status, psx_string};
use rebof3_emi::pack::{self, Payload};

use crate::error::guarded;

/// One replacement for `rebof3_emi_rebuild`.
///
/// Borrowed: read during the call only.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct rebof3_emi_replacement {
    /// The entry to replace.
    pub index: usize,
    /// The new payload.
    pub data: *const u8,
    /// The payload length in bytes.
    pub len: usize,
}

/// One payload for `rebof3_emi_pack_payloads`.
///
/// Borrowed: read during the call only.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct rebof3_emi_payload {
    /// The payload bytes.
    pub data: *const u8,
    /// The payload length in bytes.
    pub len: usize,
    /// The entry type.
    pub file_type: u16,
    /// Zero.
    pub reserved: u16,
    /// The load address.
    pub ram_ptr: u32,
}

/// Writes one entry of a path-opened archive to `path`.
///
/// # Safety
/// `archive` is a live handle from `psx_emi_open_path` and `path` points to `path_len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rebof3_emi_extract_entry(archive: *const psx_emi, index: usize, path: *const c_char, path_len: usize) -> psx_status {
    guarded(|| {
        let archive = unsafe { handle_ref(archive, "archive")? };
        archive.archive.extract_entry(index, unsafe { path_arg(path, path_len)? }).map_err(map_emi)?;
        Ok(())
    })
}

/// Writes every entry of a path-opened archive into `directory`; `typed` names the files by type.
///
/// # Safety
/// `archive` is a live handle from `psx_emi_open_path` and `directory` points to `directory_len`
/// readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rebof3_emi_extract_all(archive: *const psx_emi, directory: *const c_char, directory_len: usize, typed: bool) -> psx_status {
    guarded(|| {
        let archive = unsafe { handle_ref(archive, "archive")? };
        archive.archive.extract_all(unsafe { path_arg(directory, directory_len)? }, typed).map_err(map_emi)?;
        Ok(())
    })
}

/// The container's `emi.json` text. Free it with `psx_string_free`.
///
/// # Safety
/// `archive` is a live handle and `out_json` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rebof3_emi_manifest_json(archive: *const psx_emi, typed: bool, out_json: *mut psx_string) -> psx_status {
    guarded(|| {
        let archive = unsafe { handle_ref(archive, "archive")? };
        *unsafe { out_ref(out_json, "out_json")? } = owned_string(rebof3_formats::registry::manifest_json(&archive.archive, &archive.bytes, typed));
        Ok(())
    })
}

/// Rebuilds a container from its entries, replacing the payloads named in `replacements`. Free the
/// result with `psx_buffer_free`.
///
/// # Safety
/// `container` points to `container_len` readable bytes, each replacement points to its `len`
/// readable bytes, and `out_bytes` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rebof3_emi_rebuild(
    container: *const u8,
    container_len: usize,
    replacements: *const rebof3_emi_replacement,
    replacement_count: usize,
    out_bytes: *mut psx_buffer,
) -> psx_status {
    guarded(|| {
        let container = unsafe { input(container, container_len, "container")? };
        let replacements = unsafe { array(replacements, replacement_count, "replacements")? };
        let out_bytes = unsafe { out_ref(out_bytes, "out_bytes")? };
        let mut pairs = Vec::with_capacity(replacements.len());
        for replacement in replacements {
            pairs.push((replacement.index, unsafe { input(replacement.data, replacement.len, "replacement data")? }));
        }
        *out_bytes = owned_buffer(pack::rebuild(container, &pairs)?);
        Ok(())
    })
}

/// Packs payloads into a new container in memory. Free the result with `psx_buffer_free`.
///
/// # Safety
/// Each payload points to its `len` readable bytes and `out_bytes` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rebof3_emi_pack_payloads(archive_version: u32, payloads: *const rebof3_emi_payload, payload_count: usize, out_bytes: *mut psx_buffer) -> psx_status {
    guarded(|| {
        let payloads = unsafe { array(payloads, payload_count, "payloads")? };
        let out_bytes = unsafe { out_ref(out_bytes, "out_bytes")? };
        let mut inputs = Vec::with_capacity(payloads.len());
        for payload in payloads {
            inputs.push(Payload { bytes: unsafe { input(payload.data, payload.len, "payload data")? }, file_type: payload.file_type, ram_ptr: payload.ram_ptr });
        }
        *out_bytes = owned_buffer(pack::pack_payloads(archive_version, &inputs)?);
        Ok(())
    })
}
