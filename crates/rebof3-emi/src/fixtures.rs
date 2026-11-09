//! A tiny archive builder for the tests of the crates built on this one.

use alloc::vec;
use alloc::vec::Vec;

use crate::emi::{HEADER_SIZE, MAGIC, SECTOR_SIZE, TOC_ENTRY_SIZE};

/// An archive of `(type, payload)` entries: the retail layout, payloads on sector boundaries.
pub fn archive(entries: &[(u16, &[u8])]) -> Vec<u8> {
    let entries: Vec<(u16, u32, &[u8])> =
        entries.iter().map(|(file_type, payload)| (*file_type, 0, *payload)).collect();
    archive_at(&entries)
}

/// An archive of `(type, RAM pointer, payload)` entries.
pub fn archive_at(entries: &[(u16, u32, &[u8])]) -> Vec<u8> {
    let sector = SECTOR_SIZE as usize;
    let mut out = vec![0u8; sector];
    out[0..4].copy_from_slice(&(entries.len() as u32).to_le_bytes());
    out[8..16].copy_from_slice(MAGIC);
    for (index, (file_type, ram_ptr, payload)) in entries.iter().enumerate() {
        let at = (HEADER_SIZE + index as u64 * TOC_ENTRY_SIZE) as usize;
        out[at..at + 4].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        out[at + 4..at + 8].copy_from_slice(&ram_ptr.to_le_bytes());
        out[at + 12..at + 14].copy_from_slice(&file_type.to_le_bytes());
    }
    for (_, _, payload) in entries {
        out.extend_from_slice(payload);
        out.resize(out.len().div_ceil(sector) * sector, 0);
    }
    out
}
