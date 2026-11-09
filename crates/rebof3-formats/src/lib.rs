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
//! | [`clut`] | CLUT block | `wiki: formats/graphics#palette` |
//! | [`vab`], [`sep`], [`cue`] | sound bank, sequence package, cue table | `wiki: formats/audio` |
//! | [`character_sprites`] | character sprites | `wiki: formats/character` |
//! | [`message`], [`cells`], [`script`], [`scene`], [`encounter`], [`map`], [`window`], [`resources`] | area formats | `wiki: formats/area` |
//! | [`residue`] | code images, reserved slots, unclassified entries | |
//!
//! [`field`] is not a format either: it reads the executable's field tables and the setup stream of an
//! area program.
//!
//! [`animation`] is not a format: it is the actor animation stream that both the character sprites and the
//! script tables hold, shared like [`text`].

#![forbid(unsafe_code)]

pub mod animation;
pub mod cells;
pub mod character_sprites;
pub mod characters;
pub mod clut;
pub mod coverage;
pub mod cue;
pub mod encounter;
pub mod error;
pub mod field;
pub mod formation;
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
pub mod sprite;
pub mod sprite_palettes;
pub mod text;
pub mod vab;
pub mod vram;
pub mod window;

pub use error::{Error, Result};
pub use registry::{Description, Format, RoundTrip, Selector, State, classify, extension};
