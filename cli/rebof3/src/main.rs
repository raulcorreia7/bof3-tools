// SPDX-License-Identifier: MIT
//! `rebof3`: the Breath of Fire III toolchain.
//!
//! One binary, four subcommands: `player`, `disc`, `emi` and `assets`. Each keeps the flags and
//! help of the command it replaced.

mod assets;
mod disc;
mod emi;
mod extract_all;
mod player;

use std::env;
use std::ffi::OsString;
use std::process::ExitCode;

const USAGE: &str = "\
rebof3: the Breath of Fire III toolchain.

Usage: rebof3 <COMMAND> [ARGS...]
       rebof3 <COMMAND> --help

Commands:
  player  browse, search, play, render, export and verify the game's audio
  disc    extract, checksum, verify and rebuild a PlayStation disc image
  emi     list, extract, pack and inspect EMI archives
  assets  extract every disc asset, or convert one asset to and from its open form

Options:
  -h, --help     print this text, or a command's own help
  -V, --version  print the version

Examples:
  rebof3 player music
  rebof3 disc extract -i \"Breath of Fire III (USA).cue\" -o build/disc
  rebof3 emi list \"BIN/BGM/BGM000.EMI\"

Every command prints its purpose, every switch with its default, and runnable
examples: rebof3 <COMMAND> --help, then rebof3 <COMMAND> <VERB> --help.
";

fn main() -> ExitCode {
    let mut args = env::args_os();
    args.next();
    let Some(command) = args.next() else {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    };
    let rest = args.collect::<Vec<OsString>>();
    match command.to_str() {
        Some("player") => player::main(rest),
        Some("disc") => disc::main(rest),
        Some("emi") => emi::main(rest),
        Some("assets") => assets::main(rest),
        Some("-h" | "--help" | "help") => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some("-V" | "--version") => {
            println!("rebof3 {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!(
                "rebof3: unknown command {command:?}; run `rebof3 --help` for the commands, or \
                 `rebof3 <COMMAND> --help` for one command\n\n{USAGE}"
            );
            ExitCode::from(2)
        }
    }
}
