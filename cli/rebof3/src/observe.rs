// SPDX-License-Identifier: MIT
//! `bof3 observe`: run the disc on a route and cache which depth and palette each texture is drawn
//! with.

use std::env;
use std::ffi::OsString;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use rebof3_observe::compare::{self, Comparison, Verdict};
use rebof3_observe::{Observation, Progress, Route, cache};

use crate::disc::{IMAGE_DIR, find_disc_image};

/// Exit codes: 0 success, 2 an error.
const EXIT_ERROR: u8 = 2;
/// Frames between progress lines.
const PROGRESS_FRAMES: u64 = 500;

const USAGE: &str = "\
bof3 observe: run the disc on a scripted route and record which depth and palette the
game draws each texture upload with. The result is a cache the editor reads; it is kept
in ROOT/build/cache/observe/<disc sha>/<route>.json and rebuilt when the route or the
machine changes.

Usage: bof3 observe --route NAME|FILE.toml [-i IMAGE.cue] [--root DIR] [--force] [-q]

Options:
  -r, --route NAME   a built-in route (new-game) or a route file (required)
  -i, --input PATH   disc image or .cue sheet (default: the image in inputs/external)
      --root DIR     the project folder whose build/cache holds the result
                     (default: the current folder)
      --force        run the route even when a fresh cache exists
  -q, --quiet        print the summary only, no progress
  -h, --help         print this text

Examples:
  bof3 observe --route new-game
  bof3 observe --route new-game -i \"Breath of Fire III (USA).cue\" --root my-mod
";

#[derive(Default)]
struct Options {
    route: Option<String>,
    input: Option<PathBuf>,
    root: Option<PathBuf>,
    force: bool,
    quiet: bool,
}

pub fn main(args: Vec<OsString>) -> ExitCode {
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("bof3 observe: {message}");
            ExitCode::from(EXIT_ERROR)
        }
    }
}

fn run(args: Vec<OsString>) -> Result<(), String> {
    if args.is_empty() || args.iter().any(|arg| matches!(arg.to_str(), Some("help" | "-h" | "--help"))) {
        print!("{USAGE}");
        return Ok(());
    }
    let options = parse(&args)?;
    let route = route(options.route.as_deref().ok_or("pass --route NAME; run `bof3 observe --help`")?)?;
    let current = env::current_dir().map_err(|error| error.to_string())?;
    let image = match options.input {
        Some(image) => image,
        None => find_disc_image(&current).map_err(|error| format!("{error}. Put the CUE/BIN set in {IMAGE_DIR}/ or pass -i IMAGE."))?,
    };
    let root = options.root.unwrap_or(current);
    let disc_sha = rebof3_observe::disc_sha(&image).map_err(|error| error.to_string())?;

    let cached = if options.force { None } else { cache::load(&root, &disc_sha, &route) };
    let observation = match cached {
        Some(observation) => {
            if !options.quiet {
                eprintln!("fresh cache: {}", cache::path(&root, &disc_sha, &route.name).display());
            }
            observation
        }
        None => {
            let quiet = options.quiet;
            let observation = rebof3_observe::run(&image, &route, &disc_sha, |progress: Progress| {
                if !quiet && progress.frame.is_multiple_of(PROGRESS_FRAMES) {
                    eprint!("\rframe {} of {}", progress.frame, progress.frames);
                    let _ = std::io::stderr().flush();
                }
                true
            })
            .map_err(|error| error.to_string())?;
            if !quiet {
                eprintln!();
            }
            let path = cache::store(&root, &observation).map_err(|error| error.to_string())?;
            if !quiet {
                eprintln!("wrote {}", path.display());
            }
            observation
        }
    };
    print!("{}", summary(&observation));
    let comparisons = compare::against_rules(&observation, &image).map_err(|error| error.to_string())?;
    print!("{}", rules(&comparisons));
    Ok(())
}

/// A built-in route by name, or a route file by path.
fn route(name: &str) -> Result<Route, String> {
    if let Some(route) = Route::built_in(name) {
        return Ok(route);
    }
    let path = Path::new(name);
    if !path.is_file() {
        let known: Vec<&str> = Route::built_in_names().collect();
        return Err(format!("no route {name:?}: name a built-in route ({}) or a route file", known.join(", ")));
    }
    let text = std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Route::parse(&text).map_err(|error| format!("{}: {error}", path.display()))
}

/// The coverage summary: the run, then the entries observed in each archive.
fn summary(observation: &Observation) -> String {
    let coverage = observation.coverage();
    let mut out = format!(
        "route {}: {} frames in {:.1} s; {} uploads, {} from an entry; {} textured draws\n",
        observation.route, observation.frames, observation.seconds, observation.uploads, observation.attributed_uploads, observation.textured_draws,
    );
    let named = observation.textures.iter().filter(|texture| texture.palette.is_some()).count();
    let indexed = observation.textures.iter().filter(|texture| texture.clut.is_some()).count();
    out.push_str(&format!("{} entries observed in {} archives; {} ways drawn, {named} of {indexed} CLUT rows named\n", coverage.values().sum::<usize>(), coverage.len(), observation.textures.len(),));
    for (archive, entries) in coverage {
        out.push_str(&format!("  {archive}  {entries}\n"));
    }
    out
}

/// How the observed joins compare with the draw rules, with every disagreement on its own line.
fn rules(comparisons: &[Comparison]) -> String {
    let agree = comparisons.iter().filter(|comparison| comparison.verdict == Verdict::Agrees).count();
    let mut out = format!("rules: {} rows where a rule and a draw both name the texture, {agree} agree\n", comparisons.len());
    for comparison in comparisons.iter().filter(|comparison| comparison.verdict != Verdict::Agrees) {
        let drawn = match &comparison.verdict {
            Verdict::OtherRows(rows) => {
                let rows: Vec<String> = rows.iter().map(|[x, y]| format!("{x},{y}")).collect();
                format!("drawn with CLUT {}", rows.join(" "))
            }
            Verdict::OtherDepth(bits) => {
                let bits: Vec<String> = bits.iter().map(u32::to_string).collect();
                format!("drawn at {}-bit", bits.join("/"))
            }
            Verdict::Agrees => String::new(),
        };
        out.push_str(&format!("  {} #{}: rule {}-bit CLUT {},{} ({}); {drawn}\n", comparison.archive, comparison.slot, comparison.depth.bits(), comparison.clut.x, comparison.clut.y, comparison.rule));
    }
    out
}

fn parse(args: &[OsString]) -> Result<Options, String> {
    let mut options = Options::default();
    let mut index = 0;
    while index < args.len() {
        let flag = args[index].to_str().ok_or("options must be valid UTF-8")?;
        match flag {
            "-q" | "--quiet" => options.quiet = true,
            "--force" => options.force = true,
            "-r" | "--route" | "-i" | "--input" | "--root" => {
                index += 1;
                let value = args.get(index).and_then(|value| value.to_str()).ok_or_else(|| format!("option {flag} needs a value; run `bof3 observe --help`"))?;
                match flag {
                    "-r" | "--route" => options.route = Some(value.to_string()),
                    "-i" | "--input" => options.input = Some(PathBuf::from(value)),
                    _ => options.root = Some(PathBuf::from(value)),
                }
            }
            value => return Err(format!("unknown option: {value}; run `bof3 observe --help`")),
        }
        index += 1;
    }
    Ok(options)
}
