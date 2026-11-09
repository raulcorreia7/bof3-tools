//! The game's text as the format modules share it: where the strings sit in a payload, the strings
//! of a payload as JSON, the strings a documented table places, and the source of a string.
//!
//! The byte encodings themselves are `rebof3_emi::text::codec`, a primitive the game tables and
//! enemy records share. A message bank is the `message` module; any other payload goes through
//! [`strings`]. [`discover`] holds the scan rules (`wiki: data/text`).

pub mod discover;
pub mod known;
pub mod source;
pub mod strings;
