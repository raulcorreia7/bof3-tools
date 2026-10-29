//! The game's text: the byte encodings of the message engine and the name fields, and the rules that find
//! the strings in an entry.
//!
//! [`codec`] reads and writes the encodings. The name fields of the game tables and the enemy records use the
//! name encoding, so every name keeps its colour codes and any other byte it holds.

pub mod codec;
pub mod discover;
