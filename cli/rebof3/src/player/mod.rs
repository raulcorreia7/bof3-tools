// SPDX-License-Identifier: MIT
//! `rebof3 player`: the monorepo's thin caller into `bof3player`.
//!
//! The command implementation and every game-facing flag live in the
//! `bof3player` binary/library crate. This module adds the monorepo's BoF3 name
//! adapter crate and calls the same entry point as the standalone binary.

use std::ffi::OsString;
use std::path::Path;
use std::process::ExitCode;

/// Run `bof3player` with the monorepo's BoF3 name tables.
pub fn main(args: Vec<OsString>) -> ExitCode {
    bof3player::cli::main("rebof3 player", args, rebof3_audio_names::game_names())
}

/// Render every playable item that matches `query` as WAV files into `out_dir`.
pub fn render_matching(disc: &Path, query: &str, out_dir: &Path) -> Result<usize, String> {
    bof3player::cli::render_matching(disc, query, out_dir, rebof3_audio_names::game_names())
}
