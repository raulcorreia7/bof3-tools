//! The editor's rules as pure functions over bytes: where each archive entry sits in the game catalog
//! ([`catalog`]), the record layouts of the fixed tables ([`schema`]), and the edit store with its undo
//! history and the plan that rebuilds the edited archives ([`edits`]). No file is read or written here;
//! the editor reads the disc and saves the project.

#![forbid(unsafe_code)]

pub mod archive;
pub mod catalog;
pub mod edits;
pub mod schema;
