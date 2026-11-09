//! Decoders applied to one entry of an EMI container on disk.
//!
//! The format modules in `psx-formats` work on bytes and know
//! nothing of the container; this module joins them to [`Archive`].

use std::path::Path;

use crate::emi::{Archive, Entry, read_entry};
use crate::error::{Error, Result};
use psx_formats::clut::Palette;
use psx_formats::sep::Package;
use psx_formats::vab::{BODY_TYPE, Bank};

/// The container entry holding the body of the VAB header at `index`: the
/// type-7 entry that shares its load argument.
pub fn vab_body_index(entries: &[Entry], index: usize) -> Result<usize> {
    let entry = entries.get(index).ok_or(Error::EntryOutOfRange { index, count: entries.len() })?;
    let bodies: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_other, body)| body.file_type == BODY_TYPE && body.ram_ptr == entry.ram_ptr)
        .map(|(index, _body)| index)
        .collect();
    match bodies.as_slice() {
        [body] => Ok(*body),
        _ => Err(Error::InvalidArchive("a VAB header needs one body entry with its load argument")),
    }
}

/// Parses entry `index` as a VAB header, with the body entry that shares its
/// load argument beside it.
pub fn vab(archive_path: impl AsRef<Path>, index: usize) -> Result<Bank> {
    let path = archive_path.as_ref();
    let archive = Archive::open(path)?;
    let body = vab_body_index(archive.entries(), index)?;
    Ok(Bank::parse(&read_entry(path, index)?, &read_entry(path, body)?)?)
}

/// Decodes entry `index` as a SEP package.
pub fn sep(archive_path: impl AsRef<Path>, index: usize) -> Result<Package> {
    Ok(Package::parse(&read_entry(archive_path, index)?)?)
}

/// Decodes entry `index` as a CLUT block.
pub fn palette(archive_path: impl AsRef<Path>, index: usize) -> Result<Palette> {
    Ok(Palette::decode(&read_entry(archive_path, index)?)?)
}
