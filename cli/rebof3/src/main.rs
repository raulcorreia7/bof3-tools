// SPDX-License-Identifier: MIT
//! `bof3`: the Breath of Fire III toolchain.
//!
//! Subcommands: `disc` and `emi`, plus `assets` in the default build. Each keeps
//! the flags and help of the command it replaced.

#[cfg(feature = "assets")]
mod assets;
mod disc;
mod emi;
#[cfg(feature = "assets")]
mod extract_all;

use std::env;
use std::ffi::OsString;
use std::process::ExitCode;

#[cfg(feature = "assets")]
const USAGE: &str = "\
bof3: the Breath of Fire III toolchain.

Usage: bof3 <COMMAND> [ARGS...]
       bof3 <COMMAND> --help

Commands:
  disc    extract, checksum, verify and rebuild a PlayStation disc image
  emi     list, extract, pack and inspect EMI archives
  assets  extract every disc asset, or convert one asset to and from its open form

Options:
  -h, --help     print this text, or a command's own help
  -V, --version  print the version

Examples:
  bof3 disc extract -i \"Breath of Fire III (USA).cue\" -o build/disc
  bof3 emi list \"BIN/BGM/BGM000.EMI\"

Every command prints its purpose, every switch with its default, and runnable
examples: bof3 <COMMAND> --help, then bof3 <COMMAND> <VERB> --help.
";

#[cfg(not(feature = "assets"))]
const USAGE: &str = "\
bof3: disc and EMI tools for game setup.

Usage: bof3 <COMMAND> [ARGS...]

Commands:
  disc    extract, checksum, verify and rebuild a PlayStation disc image
  emi     list, extract, pack and inspect EMI archives

Options:
  -h, --help     print this text
  -V, --version  print the version

Asset conversion requires the full build: cargo build --release -p rebof3 --features assets
Run bof3 <COMMAND> --help for command options.
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
        Some("disc") => disc::main(rest),
        Some("emi") => emi::main(rest),
        #[cfg(feature = "assets")]
        Some("assets") => assets::main(rest),
        Some("-h" | "--help" | "help") => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some("-V" | "--version") => {
            println!("bof3 {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!(
                "bof3: unknown command {command:?}; run `bof3 --help` for the commands, or \
                 `bof3 <COMMAND> --help` for one command\n\n{USAGE}"
            );
            ExitCode::from(2)
        }
    }
}
