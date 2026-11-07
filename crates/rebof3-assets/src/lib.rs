//! Extracts every game asset from the Breath of Fire III disc.
//!
//! [`extract::run`] reads the disc through `psx-cd`, classifies each file and
//! each EMI slot with `rebof3-emi`, and writes one tree of assets plus
//! `assets.jsonl`. [`kind`] lists the kinds, [`report`] the table and summary,
//! [`terrain`] the area meshes, [`gltf`] the binary glTF writer, [`tables`] the
//! fixed data tables, [`text`] the game's text (the codec, the message banks and the
//! strings of the extracted disc), [`sprites`] the sprite banks' palettes,
//! and [`disc`] the patched disc image, whose writer is `psx_cd::patch`.

pub mod codec;
pub mod coverage;
pub mod decode;
pub mod disc;
pub mod error;
pub mod extract;
pub mod gltf;
pub mod import;
pub mod kind;
pub mod report;
pub mod sound;
pub mod soundfont;
pub mod sprites;
pub mod tables;
pub mod terrain;
pub mod text;

pub use error::{Error, Result};
