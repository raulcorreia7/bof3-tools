//! Extracts every game asset from the Breath of Fire III disc.

pub mod codec;
pub mod coverage;
pub mod decode;
pub mod disc;
pub mod error;
pub mod extract;
pub mod import;
pub mod kind;
pub mod report;
pub mod sound;
pub mod sprites;
pub mod tables;
pub mod text;

pub use error::{Error, Result};
