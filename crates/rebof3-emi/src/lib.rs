//! The EMI container and the game formats stored in it.
//!
//! The container table and the type-8 cue records come from the `emi` crate, which reads them
//! from bytes alone (`no_std + alloc`); this crate binds them to the game's formats.
//!
//! | Module | Format | Spec |
//! | --- | --- | --- |
//! | [`emi`], `pack`, `manifest` | container, repacking, `emi.json` | `wiki: formats/emi` |
//! | `lz77` | type-1 payload codec | `wiki: formats/lz77` |
//! | [`cue`] | type-8 cue tables | `wiki: formats/audio#emi-type-8-cue-records` |
//! | `area` | script banks, scene tables, map bundles | `wiki: formats/area` |
//! | `entry` | decoders applied to one entry on disk | |
//! | `replay` | the VRAM a scene's archive loads leave behind | `wiki: concepts/loading#graphics-to-vram` |
//!
//! The PlayStation formats inside the entries (VAB, SEP, VRAM, CLUT) live in `psx-formats`. The
//! crate is `no_std + alloc` without the `std` feature, which keeps [`emi`] (the container table
//! read from bytes) and [`cue`].

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

#[cfg(feature = "std")]
pub mod area;
#[cfg(feature = "std")]
pub mod character_bank;
#[cfg(feature = "std")]
pub mod characters;
pub mod cue;
pub mod emi;
#[cfg(feature = "std")]
pub mod encounter;
#[cfg(feature = "std")]
pub mod entry;
mod error;
#[cfg(any(test, feature = "fixtures"))]
pub mod fixtures;
#[cfg(feature = "std")]
pub mod game_tables;
#[cfg(feature = "std")]
pub mod lz77;
#[cfg(feature = "std")]
pub mod manifest;
#[cfg(feature = "std")]
pub mod pack;
#[cfg(feature = "std")]
mod record;
#[cfg(feature = "std")]
pub mod replay;
#[cfg(feature = "std")]
pub mod text;
#[cfg(feature = "std")]
pub mod texture;
#[cfg(feature = "std")]
pub mod verify;

pub use crate::emi::{Archive, Entry, TYPE_CUE, TYPE_LZ77};
#[cfg(feature = "std")]
pub use crate::emi::{TYPE_VRAM, archives_under, ensure_parent, entry_name, read_entry, type_extension};
#[cfg(feature = "std")]
pub use area::{map, scene, script};
pub use error::{Error, Result};
#[cfg(feature = "std")]
pub use manifest::manifest_entries;
#[cfg(feature = "std")]
pub use pack::{
    PackEntry, PackFolderOptions, Payload, glob_match, guess_type, pack, pack_folder, pack_manifest,
    pack_manifest_from, pack_payloads,
};
