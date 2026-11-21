//! The editor's rules as pure functions over bytes: where each archive entry sits in the game catalog
//! ([`catalog`]), the record layouts of the fixed tables ([`schema`]), and the edit store with its undo

#![forbid(unsafe_code)]

pub mod archive;
pub mod catalog;
pub mod edits;
pub mod schema;
