// SPDX-License-Identifier: MIT
use crate::parse;
use clap::Parser;
use psx_formats::clut;
use psx_formats::vram::{Descriptor, Plane, UPLOAD_TYPE, decode};
use rebof3_emi::{Archive, PackEntry, PackFolderOptions, ensure_parent, entry_name, guess_type, manifest_entries, pack, pack_folder};
use rebof3_formats::coverage;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::str::FromStr;

/// Exit codes: 0 success, 1 a measured negative (a reported problem), 2 an error.
const EXIT_OK: i32 = 0;
const EXIT_DIFFERS: i32 = 1;
const EXIT_ERROR: i32 = 2;

pub fn main(args: Vec<OsString>) -> ExitCode {
    match run(args) {
        Ok(code) => ExitCode::from(code as u8),
        Err(message) => {
            eprintln!("bof3 emi: {message}");
            ExitCode::from(EXIT_ERROR as u8)
        }
    }
}

fn run(args: Vec<OsString>) -> Result<i32, String> {
    let args: Vec<String> = args.into_iter().map(|arg| arg.to_string_lossy().into_owned()).collect();
    let Some(first) = args.first() else {
        print_usage();
        return Ok(EXIT_OK);
    };
    if matches!(first.as_str(), "help" | "-h" | "--help") {
        print_usage();
        return Ok(EXIT_OK);
    }
    let rest = &args[1..];
    if rest.iter().any(|arg| matches!(arg.as_str(), "help" | "-h" | "--help")) {
        print!("{}", help_for(&args));
        return Ok(EXIT_OK);
    }
    let args: Vec<OsString> = rest.iter().map(OsString::from).collect();
    let command = match first.as_str() {
        "extract" | "pack" | "list" | "vram" | "clut" | "vab" | "sep" | "coverage" => first.as_str(),
        _ => "extract",
    };
    let result = match first.as_str() {
        "extract" => run_extract(&args),
        "pack" => run_pack(&args),
        "list" => run_list(&args),
        "vram" => run_vram(&args),
        "clut" => run_clut(&args),
        "vab" => run_vab(&args),
        "sep" => run_sep(&args),
        "coverage" => run_coverage(&args),
        option if option.starts_with('-') => run_extract(&args_prefixed(&args, option)),
        path if Path::new(path).exists() || path.to_ascii_lowercase().ends_with(".emi") => run_extract(&args_prefixed(&args, path)),
        unknown => Err(format!(
            "unknown command {unknown:?}; run `bof3 emi --help` for the commands, or name an .EMI \
             archive to extract it"
        )),
    };
    result.map_err(|message| if message.contains("--help") { message } else { format!("{message}; run `bof3 emi {command} --help`") })
}

/// The full argument list with `first` in front of the already-split `rest`.
fn args_prefixed(rest: &[OsString], first: &str) -> Vec<OsString> {
    std::iter::once(OsString::from(first)).chain(rest.iter().cloned()).collect()
}

/// `bof3 emi extract`.
#[derive(Parser)]
struct ExtractArgs {
    #[arg(short = 'e', long = "extensions", visible_alias = "typed-extensions")]
    typed: bool,
    #[arg(short, long)]
    quiet: bool,
    #[arg(short, long = "dry-run")]
    dry_run: bool,
    #[arg(long)]
    print_manifest: bool,
    #[arg(short, long)]
    input: Option<PathBuf>,
    #[arg(short, long)]
    output: Option<PathBuf>,
    #[arg(short = 'n', long = "index")]
    explicit_index: Option<usize>,
    #[arg(short = 'J', long = "manifest-json", num_args = 0..=1)]
    manifest: Option<Option<PathBuf>>,
    positionals: Vec<OsString>,
}

/// The one archive a command takes, from `-i` or the only positional.
fn archive_of(command: &str, input: Option<PathBuf>, mut positionals: Vec<PathBuf>) -> Result<PathBuf, String> {
    if input.is_some() && !positionals.is_empty() || positionals.len() > 1 {
        return Err(format!("{command} takes one archive: drop the extra -i or positional; run `bof3 emi {command} --help`."));
    }
    input.or(positionals.pop()).ok_or_else(|| format!("{command} needs an archive: pass it with -i ARCHIVE or as the only positional; run `bof3 emi {command} --help`."))
}

/// The archive and the entry index of `clut`, `vab` and `sep`: `-i ARCHIVE INDEX` or `ARCHIVE INDEX`.
fn archive_and_index(command: &str, input: Option<PathBuf>, positionals: &[OsString]) -> Result<(PathBuf, usize), String> {
    let (archive, index) = match (input, positionals) {
        (Some(input), [index]) => (input, index),
        (None, [archive, index]) => (PathBuf::from(archive), index),
        (Some(_), _) => return Err(format!("{command} takes one index: pass it as INDEX; run `bof3 emi {command} --help`.")),
        (None, _) => return Err(format!("{command} needs an archive and an index: use `-i ARCHIVE INDEX` or `ARCHIVE INDEX`; run `bof3 emi {command} --help`.")),
    };
    Ok((archive, parse_num(index, &format!("{command} entry index"))?))
}

fn run_extract(args: &[OsString]) -> Result<i32, String> {
    let options: ExtractArgs = parse(args)?;
    let positional_index = match (&options.input, options.positionals.len()) {
        (Some(_), 0) => None,
        (Some(_), 1) => Some(parse_num(&options.positionals[0], "extract index")?),
        (Some(_), _) => {
            return Err("extract takes one archive and one index: pass the index as INDEX or -n INDEX, \
                        and drop the extra argument; run `bof3 emi extract --help`."
                .into());
        }
        (None, 1 | 2) => options.positionals.get(1).map(|value| parse_num(value, "extract index")).transpose()?,
        (None, _) => {
            return Err("extract needs an archive: pass it with -i ARCHIVE or as the only positional; \
                        run `bof3 emi extract --help`."
                .into());
        }
    };
    let source = match options.input {
        Some(input) => input,
        None => PathBuf::from(&options.positionals[0]),
    };
    if options.explicit_index.is_some() && positional_index.is_some() {
        return Err("extract index was provided twice: pass it once, as INDEX or -n INDEX.".into());
    }
    let entry_index = options.explicit_index.or(positional_index);
    if entry_index.is_some() && (options.manifest.is_some() || options.print_manifest) {
        return Err("manifest output needs every entry: drop -J or the index.".into());
    }
    let archive = Archive::open(&source).map_err(|error| format!("cannot read {}: {error}; check the path, or run `bof3 emi list {}` first", source.display(), source.display()))?;
    let default_output = source.with_extension("");
    if let Some(entry_index) = entry_index {
        let entry = archive.entries().get(entry_index).ok_or_else(|| format!("entry index {entry_index} is out of range (archive has {} entries)", archive.entries().len()))?;
        let output = match options.output {
            Some(path) if path.is_dir() => path.join(entry_name(entry_index, entry.file_type, options.typed)),
            Some(path) => path,
            None => default_output.join(entry_name(entry_index, entry.file_type, options.typed)),
        };
        if !options.dry_run {
            archive.extract_entry(entry_index, output).map_err(|error| error.to_string())?;
        }
        return Ok(EXIT_OK);
    }

    let output = options.output.unwrap_or(default_output);
    if !options.dry_run {
        archive.extract_all(&output, options.typed).map_err(|error| error.to_string())?;
    }
    let manifest = (options.print_manifest || options.manifest.is_some()).then(|| archive.manifest_json(options.typed));
    if options.print_manifest {
        println!("{}", manifest.as_deref().unwrap());
    }
    if let Some(requested) = options.manifest {
        let mut path = requested.unwrap_or_else(|| output.join("emi.json"));
        if path.is_dir() {
            path.push("emi.json");
        }
        if !options.dry_run {
            ensure_parent(&path).map_err(|error| error.to_string())?;
            fs::write(path, manifest.unwrap()).map_err(|error| error.to_string())?;
        }
    }
    if !options.quiet && !options.dry_run {
        eprintln!("extracted {} entries", archive.entries().len());
    }
    Ok(EXIT_OK)
}

/// `bof3 emi pack`.
#[derive(Parser)]
struct PackArgs {
    #[arg(short, long)]
    output: Option<PathBuf>,
    #[arg(short, long = "dry-run")]
    dry_run: bool,
    #[arg(short, long)]
    quiet: bool,
    #[arg(short = 'k', long = "keep-original", overrides_with = "no_keep")]
    _keep: bool,
    #[arg(short = 'K', long = "no-keep", overrides_with = "_keep")]
    no_keep: bool,
    #[arg(short = 't', long = "type")]
    default_type: Option<u16>,
    #[arg(short = 'I', long)]
    include: Vec<String>,
    #[arg(short = 'X', long)]
    exclude: Vec<String>,
    #[arg(short = 'J', long = "manifest-json", num_args = 0..=1)]
    manifest: Option<Option<PathBuf>>,
    #[arg(short, long = "input", num_args = 1..)]
    inputs: Vec<PathBuf>,
    folder: Vec<PathBuf>,
}

/// What `pack` does, as the arguments name it.
struct PackOptions {
    output: Option<PathBuf>,
    dry_run: bool,
    keep: bool,
    default_type: Option<u16>,
    include: Vec<String>,
    exclude: Vec<String>,
    manifest: Option<Option<PathBuf>>,
    folder: Option<PathBuf>,
    inputs: Vec<PackEntry>,
}

fn parse_pack_args(args: &[OsString]) -> Result<PackOptions, String> {
    let args: PackArgs = parse(args)?;
    if args.folder.len() > 1 || (!args.folder.is_empty() && !args.inputs.is_empty()) {
        return Err(format!("unexpected positional in pack mode: {}", args.folder[args.folder.len() - 1].display()));
    }
    let inputs = args.inputs.into_iter().map(|path| PackEntry { file_type: guess_type(&path).or(args.default_type).unwrap_or(255), path, ram_ptr: 0 }).collect();
    Ok(PackOptions {
        output: args.output,
        dry_run: args.dry_run,
        keep: !args.no_keep,
        default_type: args.default_type,
        include: args.include,
        exclude: args.exclude,
        manifest: args.manifest,
        folder: args.folder.into_iter().next(),
        inputs,
    })
}

fn run_pack(args: &[OsString]) -> Result<i32, String> {
    let options = parse_pack_args(args)?;
    let output = options.output.clone().ok_or("pack requires -o <archive.EMI>")?;
    if options.folder.is_some() && !options.inputs.is_empty() {
        return Err("pack accepts either a folder or -i inputs, not both".into());
    }
    if options.manifest.is_some() && options.folder.is_none() {
        return Err("manifest input is only supported when packing a folder".into());
    }

    // The containment root the folder modes validated their inputs against; the
    // cleanup rechecks every path against it. `-i` inputs have no folder: the
    // caller named those files, so their scope stays caller-selected.
    let (keep, dry_run) = (options.keep, options.dry_run);
    let (packed_paths, cleanup_root) = match options.folder.clone() {
        Some(folder) => {
            if !folder.is_dir() {
                return Err(format!("input folder does not exist: {}", folder.display()));
            }
            let root = fs::canonicalize(&folder).map_err(|error| format!("{}: {error}", folder.display()))?;
            (pack_from_folder(&output, &folder, options)?, Some(root))
        }
        None => (pack_from_inputs(&output, &options)?, None),
    };
    if !keep && !dry_run {
        cleanup_inputs(packed_paths, cleanup_root.as_deref())?;
    }
    Ok(EXIT_OK)
}

/// Packs a folder, by its manifest or by its selected files; returns the packed paths.
fn pack_from_folder(output: &Path, folder: &Path, options: PackOptions) -> Result<Vec<PathBuf>, String> {
    if let Some(requested) = options.manifest {
        let manifest = requested.unwrap_or_else(|| folder.join("emi.json"));
        // The manifest loads and validates once; these entries serve the
        // pack, the dry run and the cleanup, which never rereads it.
        let (entries, archive_version) = manifest_entries(folder, &manifest).map_err(|error| format!("cannot read the manifest {}: {error}; pass -J FILE or drop -J", manifest.display()))?;
        if !options.dry_run {
            pack(output, &entries, archive_version).map_err(|error| error.to_string())?;
        }
        return Ok(entries.into_iter().map(|entry| entry.path).collect());
    }
    let mut exclude = options.exclude;
    if !exclude.iter().any(|pattern| pattern == "emi.json") {
        exclude.push("emi.json".into());
    }
    let folder_options = PackFolderOptions { default_type: options.default_type, include_patterns: options.include, exclude_patterns: exclude };
    let paths = enumerate_paths(folder, &folder_options)?;
    if !options.dry_run {
        pack_folder(output, folder, &folder_options).map_err(|error| error.to_string())?;
    }
    Ok(paths)
}

/// Packs the files named with `-i`; returns their paths.
fn pack_from_inputs(output: &Path, options: &PackOptions) -> Result<Vec<PathBuf>, String> {
    if options.inputs.is_empty() {
        return Err("no input files or folder provided".into());
    }
    for input in &options.inputs {
        let metadata = fs::metadata(&input.path).map_err(|error| format!("{}: {error}", input.path.display()))?;
        if !metadata.is_file() {
            return Err(format!("{} is not a regular file", input.path.display()));
        }
    }
    if !options.dry_run {
        pack(output, &options.inputs, 0).map_err(|error| error.to_string())?;
    }
    Ok(options.inputs.iter().map(|entry| entry.path.clone()).collect())
}

fn enumerate_paths(folder: &Path, options: &PackFolderOptions) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(folder).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        if !entry.file_type().map_err(|error| error.to_string())?.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let included = options.include_patterns.is_empty() || options.include_patterns.iter().any(|pattern| rebof3_emi::glob_match(pattern, &name));
        let excluded = options.exclude_patterns.iter().any(|pattern| rebof3_emi::glob_match(pattern, &name));
        if included && !excluded {
            // Entries are recorded resolved, so the cleanup step can compare a
            // path against the identity that was validated and packed.
            paths.push(fs::canonicalize(entry.path()).map_err(|error| format!("{}: {error}", entry.path().display()))?);
        }
    }
    paths.sort();
    if paths.is_empty() {
        return Err("folder contains no selected regular files".into());
    }
    Ok(paths)
}

/// Deletes the inputs the pack step already owns, once each.
///
/// `owned` is that list, so repeated manifest entries collapse here instead of
/// through a second collection of the payloads. Each path is re-resolved first:
/// one that has disappeared, no longer resolves to itself, is no longer a
/// regular file, or no longer lies inside `folder` was replaced while the
/// archive was written. Such a path is refused by name and nothing is deleted,
/// which leaves the completed archive in place. `-i` inputs pass no folder: the
/// caller named them, and only their own identity is rechecked.
fn cleanup_inputs(owned: Vec<PathBuf>, folder: Option<&Path>) -> Result<(), String> {
    let mut resolved_paths: Vec<PathBuf> = Vec::with_capacity(owned.len());
    let mut pending: Vec<PathBuf> = Vec::with_capacity(owned.len());
    for path in owned {
        let resolved = fs::canonicalize(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        if !resolved.is_file() {
            return Err(format!("{} is not a regular file", path.display()));
        }
        if let Some(root) = folder
            && (resolved != path || !resolved.starts_with(root))
        {
            return Err(format!("{} no longer resolves inside {}", path.display(), root.display()));
        }
        if resolved_paths.contains(&resolved) {
            continue;
        }
        resolved_paths.push(resolved);
        pending.push(path);
    }
    for path in pending {
        fs::remove_file(&path).map_err(|error| format!("cannot remove {}: {error}", path.display()))?;
    }
    Ok(())
}

fn parse_num<T: FromStr>(value: &OsStr, label: &str) -> Result<T, String> {
    value.to_str().ok_or_else(|| format!("{label} must be valid UTF-8"))?.parse().map_err(|_| format!("invalid {label}"))
}

const USAGE: &str = "\
bof3 emi: list, extract, pack and inspect EMI archives.

Usage: bof3 emi <COMMAND> [OPTIONS]
       bof3 emi ARCHIVE.EMI [OPTIONS]     extract it

Commands:
  extract   write every entry of an archive, or one entry, as files
  pack      build an archive from a folder or from named files
  list      show the entry table, as text or JSON
  vram      decode the type-3 VRAM uploads into one page set
  clut      decode one CLUT palette entry
  vab       parse one VAB sound bank entry
  sep       parse one sequence package entry
  coverage  check which formats rebuild their own bytes

Every command has focused help: bof3 emi <COMMAND> --help.
Common switches: -i/--input PATH, -o/--output PATH, -q/--quiet.

Examples:
  bof3 emi list \"BIN/BGM/BGM000.EMI\"
  bof3 emi extract -i \"BIN/BGM/BGM000.EMI\" -o build/bgm000
  bof3 emi pack -o build/test.EMI build/src
";

const EXTRACT_HELP: &str = "\
bof3 emi extract: write every entry of an archive, or one entry, as files.

Usage: bof3 emi extract [-i ARCHIVE] [-o DIR|FILE] [-n INDEX] [-e] [-J [FILE]] [-d] [-q]

  -i, --input ARCHIVE       archive to read (or pass it as the only positional)
  -o, --output PATH         DIR for every entry, FILE for one INDEX
                            (default: the archive name without .EMI)
  -n, --index INDEX         write only this entry (default: every entry)
      --extensions, -e      add each entry's format extension (default: off)
  -J, --manifest-json [FILE]
                            also write emi.json (default: DIR/emi.json); a bare -J
                            must be last, because the next argument is its path
  -d, --dry-run             write nothing (default: off)
  -q, --quiet               no completion line (default: off)

Examples:
  bof3 emi extract -i \"BIN/BGM/BGM000.EMI\" -o build/bgm000
  bof3 emi extract -i \"BIN/BGM/BGM000.EMI\" -n 3 -e -o build/one
  bof3 emi extract -i \"BIN/BGM/BGM000.EMI\" -o build/bgm000 -J
";

const PACK_HELP: &str = "\
bof3 emi pack: build an EMI archive from a folder, or from named files.

Usage: bof3 emi pack -o ARCHIVE.EMI [-i FILE...] | FOLDER [OPTIONS]

  -i, --input FILE...       files to pack, in order (default: use FOLDER)
  -o, --output ARCHIVE.EMI  archive to write (required)
  -t, --type TYPE           file type for inputs whose extension names none
                            (default: 255)
  -I, --include PATTERN     folder files to include (default: every file)
  -X, --exclude PATTERN     folder files to exclude (default: emi.json)
  -J, --manifest-json [FILE]
                            read emi.json instead of guessing types
                            (default: FOLDER/emi.json)
  -k, --keep-original       keep the input files (default)
  -K, --no-keep             delete the inputs after the archive is written
  -d, --dry-run             write nothing (default: off)
  -q, --quiet               no completion line (default: off)

Examples:
  bof3 emi pack -o build/test.EMI -i header.vh body.vb
  bof3 emi pack -o build/test.EMI -X '*.md' build/src
";

const LIST_HELP: &str = "\
bof3 emi list: show an archive's entry table.

Usage: bof3 emi list [-i ARCHIVE] [--json]

  -i, --input ARCHIVE  archive to read (or pass it as the only positional)
      --json           print one JSON object per entry, as a JSON array
                       (default: a text table)

Examples:
  bof3 emi list \"BIN/BGM/BGM000.EMI\"
  bof3 emi list -i \"BIN/BGM/BGM000.EMI\" --json
";

const VRAM_HELP: &str = "\
bof3 emi vram: decode an archive's type-3 VRAM uploads into one page set.

Usage: bof3 emi vram [-i ARCHIVE] [-o PAGE.png]

  -i, --input ARCHIVE  archive to read (or pass it as the only positional)
  -o, --output PAGE.png
                       also write the page set as a PNG (default: print only)

Examples:
  bof3 emi vram \"BIN/BGM/BGM000.EMI\"
  bof3 emi vram -i \"BIN/BGM/BGM000.EMI\" -o build/page.png
";

const CLUT_HELP: &str = "\
bof3 emi clut: decode one CLUT palette entry.

Usage: bof3 emi clut [-i ARCHIVE] INDEX [-j FILE.json] [-p FILE.png]

  -i, --input ARCHIVE  archive to read (or pass it as the first positional)
  INDEX                entry index to decode (required)
  -j, --json FILE      write the palette as JSON (default: print only)
  -p, --png FILE       write the palette as a swatch strip (default: print only)

Examples:
  bof3 emi clut \"BIN/BGM/BGM000.EMI\" 3
  bof3 emi clut -i \"BIN/BGM/BGM000.EMI\" 3 -j build/palette.json -p build/palette.png
";

const AUDIO_HELP: &str = "\
bof3 emi {vab|sep}: parse one sound bank or sequence package entry.

Usage: bof3 emi vab [-i ARCHIVE] INDEX [--dump]
       bof3 emi sep [-i ARCHIVE] INDEX [--dump]

  -i, --input ARCHIVE  archive to read (or pass it as the first positional)
  INDEX                entry index to parse (required)
      --dump           print every field of the entry (default: a summary)

Examples:
  bof3 emi vab \"BIN/BGM/BGM000.EMI\" 1
  bof3 emi sep -i \"BIN/SEQ/SEQM00.EMI\" 0 --dump
";

const COVERAGE_HELP: &str = "\
bof3 emi coverage: check which formats rebuild their own bytes.

Usage: bof3 emi coverage [-i DIR] [-o FILE] [--json]

  -i, --input DIR    folder of .EMI archives to check (default: raw/disc/BIN)
  -o, --output FILE  write the JSONL table to FILE (default: print only)
      --json         print the JSONL table instead of the text table

Examples:
  bof3 emi coverage
  bof3 emi coverage -i raw/disc/BIN -o build/coverage.jsonl
  bof3 emi coverage --json
";

/// The focused help for the command in `args`.
fn help_for(args: &[String]) -> &'static str {
    match args.first().map(String::as_str) {
        Some("extract") => EXTRACT_HELP,
        Some("pack") => PACK_HELP,
        Some("list") => LIST_HELP,
        Some("vram") => VRAM_HELP,
        Some("clut") => CLUT_HELP,
        Some("vab" | "sep") => AUDIO_HELP,
        Some("coverage") => COVERAGE_HELP,
        _ => USAGE,
    }
}

fn print_usage() {
    print!("{USAGE}");
}

/// `bof3 emi list`.
#[derive(Parser)]
struct ListArgs {
    #[arg(short, long)]
    input: Option<PathBuf>,
    #[arg(long)]
    json: bool,
    archive: Vec<PathBuf>,
}

fn run_list(args: &[OsString]) -> Result<i32, String> {
    let ListArgs { input, json, archive } = parse(args)?;
    let source = archive_of("list", input, archive)?;
    let archive = Archive::open(&source).map_err(|error| format!("cannot read {}: {error}; check the path, or run `bof3 emi list --help`", source.display()))?;
    if json {
        let entries: Vec<serde_json::Value> = archive
            .entries()
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                serde_json::json!({
                    "index": index,
                    "offset": entry.offset,
                    "ram_ptr": entry.ram_ptr,
                    "size": entry.size,
                    "type": entry.file_type,
                    "first4": entry.first4,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&entries).map_err(|error| error.to_string())?);
    } else {
        println!("{:<5} {:<10} {:<12} {:<10} {:<6} FIRST4", "IDX", "OFFSET", "RAM_PTR", "SIZE", "TYPE");
        for (index, entry) in archive.entries().iter().enumerate() {
            println!("{:<5} {:<10} {:<#12x} {:<#10x} {:<6} {:#x}", index, format!("{:#x}", entry.offset), entry.ram_ptr, entry.size, entry.file_type, entry.first4,);
        }
    }
    Ok(EXIT_OK)
}

/// `bof3 emi vram`.
#[derive(Parser)]
struct VramArgs {
    #[arg(short, long)]
    input: Option<PathBuf>,
    #[arg(short, long)]
    output: Option<PathBuf>,
    archive: Vec<PathBuf>,
}

/// Decodes an archive's type-3 uploads into one page set, optionally as PNG.
fn run_vram(args: &[OsString]) -> Result<i32, String> {
    let VramArgs { input, output, archive } = parse(args)?;
    let source = archive_of("vram", input, archive)?;
    let archive = Archive::open(&source).map_err(|error| format!("cannot read {}: {error}; check the path, or run `bof3 emi list {}`", source.display(), source.display()))?;
    let retail = fs::read(&source).map_err(|error| format!("cannot read {}: {error}; check the path and its permissions", source.display()))?;
    let mut page_set = Plane::new();
    print_uploads(&archive, &retail, &mut page_set, &source)?;
    if let Some(output) = output {
        ensure_parent(&output).map_err(|error| error.to_string())?;
        page_set.write_png(&output).map_err(|error| format!("{}: {error}", output.display()))?;
        eprintln!("wrote {}", output.display());
    }

    Ok(EXIT_OK)
}

/// Places every type-3 entry of the archive into `page_set`, printing one row
/// per upload and the totals.
fn print_uploads(archive: &Archive, retail: &[u8], page_set: &mut Plane, source: &Path) -> Result<(), String> {
    let mut uploads = 0;
    let mut rectangles = 0;
    let mut dropped = 0;
    println!("{:<5} {:<22} {:<12} {:>8} {:>8}", "IDX", "DESCRIPTOR", "RECTANGLES", "BYTES", "DROPPED");
    for (index, entry) in archive.entries().iter().enumerate() {
        if entry.file_type != UPLOAD_TYPE {
            continue;
        }
        uploads += 1;
        let payload = &retail[entry.offset as usize..(entry.offset + entry.size as u64) as usize];
        let descriptor = Descriptor::from_load_argument(entry.ram_ptr);
        let decoded = decode(payload, descriptor).map_err(|error| format!("{}#{index}: {error}", source.display()))?;
        let skipped = page_set.blit(&decoded);
        rectangles += decoded.len();
        dropped += skipped;
        println!("{:<5} {:<22} {:<12} {:>8} {:>8}", index, format!("col={} row={} per_row={}", descriptor.column, descriptor.row, descriptor.chunks_per_row), decoded.len(), entry.size, skipped,);
    }
    if uploads == 0 {
        return Err(format!("{}: no type-3 entry", source.display()));
    }
    eprintln!("{uploads} uploads, {rectangles} rectangles, {dropped} outside the frame buffer");
    Ok(())
}

/// `bof3 emi clut`.
#[derive(Parser)]
struct ClutArgs {
    #[arg(short, long)]
    input: Option<PathBuf>,
    #[arg(short, long)]
    json: Option<PathBuf>,
    #[arg(short, long)]
    png: Option<PathBuf>,
    positionals: Vec<OsString>,
}

fn run_clut(args: &[OsString]) -> Result<i32, String> {
    let ClutArgs { input, json, png, positionals } = parse(args)?;
    let (source, entry_index) = archive_and_index("clut", input, &positionals)?;
    let palette = rebof3_emi::entry::palette(&source, entry_index)
        .map_err(|error| format!("cannot read {}#{entry_index}: {error}; run `bof3 emi list {}` to see the entry table", source.display(), source.display()))?;
    if let Some(path) = json {
        ensure_parent(&path).map_err(|error| error.to_string())?;
        fs::write(&path, palette.json()).map_err(|error| error.to_string())?;
    }
    if let Some(path) = png {
        ensure_parent(&path).map_err(|error| error.to_string())?;
        fs::write(&path, palette.png_strip()).map_err(|error| error.to_string())?;
    }
    println!(
        "{entry_index}: {} color words, {} rows of {}{}",
        palette.words().len(),
        palette.rows_4bpp(),
        clut::ROW_WORDS_4BPP,
        match palette.rows_8bpp() {
            Some(_) => " (8bpp-aligned)",
            None => "",
        }
    );
    Ok(EXIT_OK)
}

/// `bof3 emi vab` and `bof3 emi sep`.
#[derive(Parser)]
struct AudioArgs {
    #[arg(short, long)]
    input: Option<PathBuf>,
    #[arg(long)]
    dump: bool,
    positionals: Vec<OsString>,
}

/// The archive, the entry index and `--dump` of an audio verb.
fn audio_args(args: &[OsString], command: &str) -> Result<(bool, PathBuf, usize), String> {
    let AudioArgs { input, dump, positionals } = parse(args)?;
    let (archive, index) = archive_and_index(command, input, &positionals)?;
    Ok((dump, archive, index))
}

fn run_vab(args: &[OsString]) -> Result<i32, String> {
    let (dump, source, index) = audio_args(args, "vab")?;
    let bank = rebof3_emi::entry::vab(&source, index).map_err(|error| format!("{}: {error}", source.display()))?;
    if dump {
        print!("{}", bank.dump());
    } else {
        println!(
            "{}#{index}: version={} header={} body={} programs={} tones={}/{} vags={} \
             mvol={} mpan={}",
            source.display(),
            bank.version,
            bank.header_size(),
            bank.body_len(),
            bank.program_count,
            bank.tone_count,
            bank.declared_tones,
            bank.vag_count,
            bank.master_vol,
            bank.master_pan,
        );
    }
    Ok(report_problems(bank.problems().into_iter().map(|problem| problem.to_string())))
}

fn run_sep(args: &[OsString]) -> Result<i32, String> {
    let (dump, source, index) = audio_args(args, "sep")?;
    let package = rebof3_emi::entry::sep(&source, index).map_err(|error| format!("{}: {error}", source.display()))?;
    if dump {
        print!("{}", package.dump());
    } else {
        println!("{}#{index}: sequences={}", source.display(), package.sequences.len());
        for (sequence, item) in package.sequences.iter().enumerate() {
            println!("seq {sequence} id={} resolution={} tempo_us={} time={}/{} events={}", item.seq_id, item.resolution, item.tempo_us, item.time_num, item.time_den, item.events.len(),);
        }
    }
    Ok(report_problems(package.problems().into_iter().map(|problem| problem.to_string())))
}

/// Prints one line per problem and fails when the parse has any.
/// Prints each problem and returns the code the caller reports: 1 when there is
/// one, because a format the retail bytes disagree with is a measured negative,
/// not a tool failure.
fn report_problems(problems: impl Iterator<Item = String>) -> i32 {
    let mut count = 0;
    for problem in problems {
        println!("problem {problem}");
        count += 1;
    }
    if count == 0 {
        EXIT_OK
    } else {
        println!("{count} problem(s)");
        EXIT_DIFFERS
    }
}

/// Where `coverage` looks for the retail archives when `--corpus` is absent,
/// relative to the working directory.
const COVERAGE_CORPUS: &str = "raw/disc/BIN";

/// `bof3 emi coverage`.
#[derive(Parser)]
struct CoverageArgs {
    #[arg(short = 'i', long = "input", visible_aliases = ["corpus"], short_alias = 'c')]
    corpus: Option<PathBuf>,
    #[arg(short = 'o', long = "output", visible_aliases = ["jsonl"], short_alias = 'j')]
    jsonl: Option<PathBuf>,
    #[arg(long)]
    json: bool,
    positional: Vec<OsString>,
}

/// Prints the format coverage scoreboard over the retail archives and, with
/// `--jsonl`, writes it as the coverage table.
fn run_coverage(args: &[OsString]) -> Result<i32, String> {
    let CoverageArgs { corpus, jsonl, json, positional } = parse(args)?;
    if !positional.is_empty() {
        return Err("coverage takes no positional: pass the archive folder with -i DIR; run `bof3 emi coverage --help`.".into());
    }
    let corpus = corpus.unwrap_or_else(|| PathBuf::from(COVERAGE_CORPUS));
    let report = coverage::check(&corpus).map_err(|error| format!("cannot read the archives under {}: {error}; pass the folder with -i DIR", corpus.display()))?;
    if json {
        print!("{}", report.jsonl());
    } else {
        print!("{}", report.table());
    }
    if let Some(path) = jsonl {
        ensure_parent(&path).map_err(|error| error.to_string())?;
        fs::write(&path, report.jsonl()).map_err(|error| format!("cannot write {}: {error}; check the folder exists and is writable", path.display()))?;
        eprintln!("wrote {}", path.display());
    }
    Ok(report_problems(report.all_failures().into_iter()))
}
