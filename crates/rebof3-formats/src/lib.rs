//! The Breath of Fire III entry formats, one self-contained module each.
//!
//! A format module reads and writes one kind of entry payload and nothing else: it depends on the
//! primitives (the EMI container's `Entry`, LZ77, the record readers, `psx-formats` and the VRAM
//! model) and never on another format module. The [`registry`] maps an entry to its format and
//! describes it; [`coverage`] runs every format's round trip over the retail archives.
//! The contract is in `docs/adr/0002-self-contained-formats.md`.
//!
//! | Module | Format | Spec |
//! | --- | --- | --- |
//! | [`vram`] | texture upload (type 3) | `wiki: formats/graphics#chunk-layout` |
//! | [`clut`] | CLUT bank | `wiki: formats/graphics#palette` |
//! | [`vab`], [`sep`], [`cue`] | sound bank, sequence package, cue table | `wiki: formats/audio` |
//! | [`sprite_bank`] | character sprite bank | `wiki: formats/character` |
//! | [`message`], [`cells`], [`script`], [`scene`], [`encounter`], [`map`], [`window`], [`resources`] | area formats | `wiki: formats/area` |
//! | [`residue`] | code images, reserved slots, unclassified entries | |

#![forbid(unsafe_code)]

pub mod cells;
pub mod clut;
pub mod coverage;
pub mod cue;
pub mod encounter;
pub mod error;
pub mod game_tables;
pub mod gltf;
pub mod identity;
pub mod joins;
pub mod map;
pub mod message;
pub mod registry;
pub mod residue;
pub mod resources;
pub mod scene;
pub mod script;
pub mod sep;
pub mod sprite_bank;
pub mod text;
pub mod vab;
pub mod vram;
pub mod window;

pub use error::{Error, Result};
pub use registry::{Description, Format, RoundTrip, Selector, State, classify, extension};
