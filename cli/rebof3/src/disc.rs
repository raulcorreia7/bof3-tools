// SPDX-License-Identifier: MIT
use psx_cd::{Entry, Image, checksum, metadata};
use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Where the disc image lives, relative to the repository root.
pub(crate) const IMAGE_DIR: &str = "inputs/external";
/// Folders searched for the disc image, in order, when no `-i` is given.
pub(crate) const IMAGE_DIRS: [&str; 2] = [IMAGE_DIR, "disk"];

/// The disc image in the first of `IMAGE_DIRS` below `base` that holds one; the error names every folder searched.
pub(crate) fn find_disc_image(base: &Path) -> Result<PathBuf, String> {
    let mut reasons = Vec::new();
    for directory in IMAGE_DIRS {
        match psx_cd::discover_image(&base.join(directory)) {
            Ok(path) => return Ok(path),
            Err(error) => reasons.push(format!("{directory}: {error}")),
        }
    }
    Err(format!("no disc image found ({})", reasons.join("; ")))
}

#[derive(Default)]
struct Options {
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    extract: Option<PathBuf>,
    project: Option<PathBuf>,
    raw_root: Option<PathBuf>,
    quiet: bool,
}

/// Exit codes: 0 success, 1 a measured negative (a verify mismatch), 2 an error.
const EXIT_OK: i32 = 0;
const EXIT_DIFFERS: i32 = 1;
const EXIT_ERROR: i32 = 2;
/// Primary volume descriptor text of every synthetic bof3 disc.
const SYSTEM_IDENTIFIER: &str = "BOF3-DISK";
const VOLUME_IDENTIFIER: &str = "BOF3 SYNTHETIC";
/// The archive extension and manifest name the bof3 disc metadata has always used.
const ARCHIVE_EXTENSION: &str = "EMI";
const MANIFEST_NAME: &str = "emi.json";

pub fn main(args: Vec<OsString>) -> ExitCode {
    match run(args) {
        Ok(code) => ExitCode::from(code as u8),
        Err(message) => {
            eprintln!("bof3 disc: {message}");
            ExitCode::from(EXIT_ERROR as u8)
        }
    }
}

fn run(args: Vec<OsString>) -> Result<i32, String> {
    if args.is_empty() {
        print_usage();
        return Ok(EXIT_OK);
    }
    let command = args[0].to_str().ok_or("the command must be valid UTF-8")?;
    let help = args.iter().any(|arg| matches!(arg.to_str(), Some("help" | "-h" | "--help")));
    if help {
        if matches!(command, "help" | "-h" | "--help") {
            print_usage();
        } else {
            print!("{}", help_for(command));
        }
        return Ok(EXIT_OK);
    }
    let options = parse_options(&args[1..])?;
    let result = match command {
        "extract" => extract(options),
        "lba-json" => lba_json(options),
        "checksum" => checksum(options),
        "rebuild" => rebuild(options),
        "verify" => verify(options),
        value => Err(format!("unknown command {value:?}; run `bof3 disc --help` for the commands")),
    };
    result.map_err(|message| {
        if message.contains("--help") || message.contains("help`") {
            message
        } else {
            format!("{message}; run `bof3 disc {command} --help`")
        }
    })
}

/// The focused help for one command.
fn help_for(command: &str) -> &'static str {
    match command {
        "extract" => EXTRACT_HELP,
        "lba-json" => LBA_JSON_HELP,
        "checksum" => CHECKSUM_HELP,
        "verify" => VERIFY_HELP,
        "rebuild" => REBUILD_HELP,
        _ => USAGE,
    }
}

fn extract(options: Options) -> Result<i32, String> {
    reject(&options, &[('x', options.extract.is_some()), ('r', options.raw_root.is_some())])?;
    let current = env::current_dir().map_err(|error| error.to_string())?;
    let input = match options.input {
        Some(path) => path,
        None => find_disc_image(&current).map_err(|error| {
            format!(
                "extract has no disc image: pass -i IMAGE.cue or put one in inputs/external ({error}); \
                     run `bof3 disc extract --help`"
            )
        })?,
    };
    let output = options.output.unwrap_or_else(|| current.join("build/extracted"));
    let project = options.project.unwrap_or_else(|| output.join(format!("{}.xml", file_stem(&input))));
    let entries = open_walk_extract(&input, &output, &project)?;
    if !options.quiet {
        eprintln!(
            "extracted {} files to {}",
            entries.iter().filter(|value| !value.is_directory).count(),
            output.display()
        );
        eprintln!("project XML: {}", project.display());
    }
    Ok(EXIT_OK)
}

fn file_stem(path: &Path) -> &str {
    path.file_stem().and_then(|value| value.to_str()).unwrap_or("disc")
}

fn rebuild(options: Options) -> Result<i32, String> {
    reject(
        &options,
        &[('x', options.extract.is_some()), ('p', options.project.is_some()), ('r', options.raw_root.is_some())],
    )?;
    let input = options.input.ok_or("rebuild needs an input folder: pass -i DIR.")?;
    let output = options.output.ok_or("rebuild needs an output file: pass -o IMAGE.iso.")?;
    psx_cd::rebuild::iso_with_labels(
        &input,
        &output,
        psx_cd::rebuild::VolumeLabels { system: SYSTEM_IDENTIFIER, volume: VOLUME_IDENTIFIER },
    )
    .map_err(display_error(&input))?;
    if !options.quiet {
        eprintln!("wrote synthetic ISO: {}", output.display());
    }
    Ok(EXIT_OK)
}

fn lba_json(options: Options) -> Result<i32, String> {
    let input = options.input.ok_or("lba-json needs an input image: pass -i IMAGE.")?;
    let output = options.output.ok_or("lba-json needs an output file: pass -o FILE.json.")?;
    let extract = options.extract.ok_or("lba-json needs the extraction folder: pass -x DIR.")?;
    let project = options.project.ok_or("lba-json needs the project XML: pass -p FILE.xml.")?;
    let raw_root = options.raw_root;
    let entries = open_walk_extract(&input, &extract, &project)?;
    metadata::write_lba_json_with(
        &output,
        &entries,
        raw_root.as_deref().unwrap_or(Path::new("processed/emi_raw")),
        &extract,
        metadata::ArchiveNaming { extension: Some(ARCHIVE_EXTENSION), manifest: MANIFEST_NAME },
    )
    .map_err(|error| error.to_string())?;
    if !options.quiet {
        eprintln!("wrote LBA JSON: {}", output.display());
    }
    Ok(EXIT_OK)
}

fn checksum(options: Options) -> Result<i32, String> {
    reject(
        &options,
        &[('p', options.project.is_some()), ('x', options.extract.is_some()), ('r', options.raw_root.is_some())],
    )?;
    let input = options.input.ok_or("checksum needs an input folder: pass -i DIR.")?;
    let output = options.output.ok_or("checksum needs an output file: pass -o FILE.json.")?;
    let current = env::current_dir().map_err(|error| error.to_string())?;
    let records = checksum::scan(&input, &current).map_err(|error| error.to_string())?;
    checksum::write(&output, &records).map_err(|error| error.to_string())?;
    if !options.quiet {
        eprintln!("wrote disk checksums: {}", output.display());
    }
    Ok(EXIT_OK)
}

fn verify(options: Options) -> Result<i32, String> {
    reject(
        &options,
        &[('p', options.project.is_some()), ('x', options.extract.is_some()), ('r', options.raw_root.is_some())],
    )?;
    let input = options.input.ok_or("verify needs an input folder: pass -i DIR.")?;
    let checksums = options.output.ok_or("verify needs the checksums file: pass -o FILE.json.")?;
    let current = env::current_dir().map_err(|error| error.to_string())?;
    let expected = checksum::read(&checksums).map_err(|error| error.to_string())?;
    let mismatches = checksum::verify(&input, &current, &expected).map_err(|error| error.to_string())?;
    if !mismatches.is_empty() {
        for mismatch in &mismatches {
            eprintln!("{mismatch}");
        }
        eprintln!("bof3 disc: verify failed with {} mismatch(es)", mismatches.len());
        return Ok(EXIT_DIFFERS);
    }
    if !options.quiet {
        eprintln!("verify passed: {} file(s) matched", expected.len());
    }
    Ok(EXIT_OK)
}

fn parse_options(args: &[OsString]) -> Result<Options, String> {
    let mut options = Options::default();
    let mut index = 0;
    while index < args.len() {
        let flag = args[index].to_str().ok_or("options must be valid UTF-8")?;
        match flag {
            "-q" | "--quiet" => options.quiet = true,
            "-i" | "--input" | "-o" | "--output" | "-x" | "--extract" | "-p" | "--project" | "-r" | "--raw-root" => {
                index += 1;
                let value = PathBuf::from(args.get(index).ok_or_else(|| {
                    format!(
                        "option {flag} needs a value, for example -o build/disc; run `bof3 disc help` for the options"
                    )
                })?);
                match flag {
                    "-i" | "--input" => options.input = Some(value),
                    "-o" | "--output" => options.output = Some(value),
                    "-x" | "--extract" => options.extract = Some(value),
                    "-p" | "--project" => options.project = Some(value),
                    "-r" | "--raw-root" => options.raw_root = Some(value),
                    _ => unreachable!(),
                }
            }
            value => return Err(format!("unknown option: {value}; run `bof3 disc help` for the options")),
        }
        index += 1;
    }
    Ok(options)
}

fn reject(_options: &Options, invalid: &[(char, bool)]) -> Result<(), String> {
    if let Some((flag, _)) = invalid.iter().find(|(_, present)| *present) {
        return Err(format!("-{flag} is not valid with this command; run `bof3 disc help` for its options"));
    }
    Ok(())
}

fn display_error(path: &Path) -> impl FnOnce(psx_cd::Error) -> String + '_ {
    move |error| format!("{}: {error}", path.display())
}

/// Extracts every file of `input` into `output` and writes the project XML beside them; returns the file count.
#[cfg(feature = "assets")]
pub(crate) fn extract_image(input: &Path, output: &Path) -> Result<usize, String> {
    let project = output.join(format!("{}.xml", file_stem(input)));
    let entries = open_walk_extract(input, output, &project)?;
    Ok(entries.iter().filter(|value| !value.is_directory).count())
}

fn open_walk_extract(input: &Path, output: &Path, project: &Path) -> Result<Vec<Entry>, String> {
    let mut image = Image::open(input).map_err(display_error(input))?;
    let entries = image.entries().map_err(display_error(input))?;
    image.extract(output).map_err(display_error(input))?;
    metadata::write_project(
        project,
        &entries,
        input.file_name().and_then(|value| value.to_str()).unwrap_or("disc.bin"),
    )
    .map_err(|error| error.to_string())?;
    Ok(entries)
}

const USAGE: &str = "\
bof3 disc: read, extract, checksum, rebuild and verify a PlayStation disc image.

Usage: bof3 disc <COMMAND> [OPTIONS]
       bof3 disc <COMMAND> --help

Commands:
  extract   write every file of an image, plus its project XML
  lba-json  write the LBA table of an extracted tree
  checksum  hash the .bin, .cue, .img and .iso files below a folder
  verify    compare those files with their checksums; exit 1 on a mismatch
  rebuild   build a synthetic ISO from top-level files

Every command has focused help: bof3 disc <COMMAND> --help.
Common switches: -i/--input PATH, -o/--output PATH, -q/--quiet.

Examples:
  bof3 disc extract -i \"Breath of Fire III (USA).cue\" -o build/disc
  bof3 disc checksum -i dumps -o build/checksums.json
  bof3 disc verify -i dumps -o build/checksums.json
";

const EXTRACT_HELP: &str = "\
bof3 disc extract: write every file of an image, plus its project XML.

Usage: bof3 disc extract [-i IMAGE.cue] [-o DIR] [-p FILE.xml] [-q]

  -i, --input PATH    disc image or .cue sheet (default: the image in
                      inputs/external, then ./disk)
  -o, --output DIR    folder to extract into (default: build/extracted)
  -p, --project FILE  project XML to write (default: DIR/IMAGE.xml)
  -q, --quiet         no completion line (default: off)

Examples:
  bof3 disc extract -i \"Breath of Fire III (USA).cue\" -o build/disc
  bof3 disc extract -q
";

const LBA_JSON_HELP: &str = "\
bof3 disc lba-json: write the LBA table of an extracted tree.

Usage: bof3 disc lba-json -i IMAGE -o FILE.json -x DIR -p FILE.xml [-r DIR] [-q]

  -i, --input PATH    disc image or .cue sheet (required)
  -o, --output FILE   JSON table to write (required)
  -x, --extract DIR   extraction folder the table describes (required)
  -p, --project FILE  project XML of that extraction (required)
  -r, --raw-root DIR  archive root (default: processed/emi_raw)
  -q, --quiet         no completion line (default: off)

Example:
  bof3 disc lba-json -i game.cue -o build/lba.json -x build/disc -p build/disc/game.xml
";

const CHECKSUM_HELP: &str = "\
bof3 disc checksum: hash the .bin, .cue, .img and .iso files below a folder.

Usage: bof3 disc checksum -i DIR -o FILE.json [-q]

  -i, --input DIR     folder that holds the disc dump (required)
  -o, --output FILE   JSON checksums to write (required)
  -q, --quiet         no completion line (default: off)

Example:
  bof3 disc checksum -i dumps -o build/checksums.json
";

const VERIFY_HELP: &str = "\
bof3 disc verify: compare the .bin, .cue, .img and .iso files below a folder with their checksums.

Usage: bof3 disc verify -i DIR -o FILE.json [-q]

  -i, --input DIR     folder that holds the disc dump (required)
  -o, --output FILE   JSON checksums to compare against (required)
  -q, --quiet         no completion line (default: off)

Exit status: 0 every file matches, 1 a mismatch, 2 an error.

Example:
  bof3 disc verify -i dumps -o build/checksums.json
";

const REBUILD_HELP: &str = "\
bof3 disc rebuild: build a synthetic ISO from top-level files.

Usage: bof3 disc rebuild -i DIR -o IMAGE.iso [-q]

  -i, --input DIR     folder whose top-level files go in the image (required)
  -o, --output FILE   ISO to write (required)
  -q, --quiet         no completion line (default: off)

Example:
  bof3 disc rebuild -i build/iso-files -o build/synthetic.iso
";

fn print_usage() {
    print!("{USAGE}");
}
