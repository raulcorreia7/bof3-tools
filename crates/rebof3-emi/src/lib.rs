//! The EMI container and the game formats stored in it.
//! | [`emi`], `pack`, `manifest` | container, repacking, `emi.json` | `wiki: formats/emi` |
//! | `lz77` | type-1 payload codec | `wiki: formats/lz77` |
//! | `record`, `text` | the record readers and the game's text codec | `wiki: data/text` |
//! | [`cue`] | type-8 cue tables | `wiki: formats/audio#emi-type-8-cue-records` |
//! | `replay` | the VRAM a scene's archive loads leave behind | `wiki: concepts/loading#graphics-to-vram` |

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

#[cfg(feature = "std")]
#[cfg(feature = "std")]
pub mod cue;
pub mod emi;
#[cfg(feature = "std")]
pub mod entry;
mod error;
#[cfg(any(test, feature = "fixtures"))]
pub mod fixtures;
#[cfg(feature = "std")]
pub mod lz77;
#[cfg(feature = "std")]
pub mod lzss;
#[cfg(feature = "std")]
pub mod manifest;
#[cfg(feature = "std")]
pub mod pack;
#[cfg(feature = "std")]
pub mod record;
#[cfg(feature = "std")]
pub mod replay;
#[cfg(feature = "std")]
#[cfg(feature = "std")]
pub mod text;
#[cfg(feature = "std")]
pub mod texture;
#[cfg(feature = "std")]
pub mod verify;

pub use crate::emi::{Archive, Entry, TYPE_CUE, TYPE_LZ77};
#[cfg(feature = "std")]
pub use crate::emi::{TYPE_VRAM, archives_under, ensure_parent, entry_name, read_entry, type_extension};
pub use error::{Error, Result};
#[cfg(feature = "std")]
pub use manifest::manifest_entries;
#[cfg(feature = "std")]
pub use pack::{
    PackEntry, PackFolderOptions, Payload, glob_match, guess_type, pack, pack_folder, pack_manifest,
    pack_manifest_from, pack_payloads,
};
