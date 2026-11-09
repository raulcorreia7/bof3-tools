// SPDX-License-Identifier: MIT
//! `assets`: extract every game asset from the disc, or convert one asset.

use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use crate::disc::find_disc_image;
use clap::Parser;
use rebof3_assets::codec::{self, Direction, Index, Job};
use rebof3_assets::coverage;
use rebof3_assets::decode;
use rebof3_assets::disc;
use rebof3_assets::extract::{self, Options, SUMMARY_FILE, TABLE_FILE};
use rebof3_assets::import::{self, TextureImport};
use rebof3_assets::kind::{AudioFormat, Kind, Mode, Selection};
use rebof3_assets::soundfont;
use rebof3_assets::text::{self, DEFAULT_DISC};
use rebof3_assets::{Error, Result};
use rebof3_formats::map::terrain;

/// Exit codes: 0 success, 1 some payloads failed to write, 2 an error.
const EXIT_OK: i32 = 0;
const EXIT_FAILURES: i32 = 1;
const EXIT_ERROR: i32 = 2;
/// Default output directory, relative to the repository root.
const DEFAULT_OUT: &str = "build/game-assets";
const USAGE: &str = "\
bof3 assets: extract the disc's assets, or convert one asset.

Usage: bof3 assets <COMMAND> [OPTIONS]
       bof3 assets <COMMAND> --help

Commands:
  extract      write the disc's assets, one file per asset
  extract-all  every asset of the disc, one folder per step, plus index.json
  coverage     report which kinds rebuild their own bytes
  kinds        list the asset kinds and what each writes
  terrain      write one map bundle's terrain as glTF or OBJ
  soundfont    write one VAB bank as a SoundFont 2 file
  tables       write the game tables of ETC/GAME.EMI entry 0 as JSON
  encounters   write one area's encounter block as JSON
  export       write one asset's open form from its stored bytes
  import       read an open form back into its stored bytes

Common switches:
  -i, --input PATH   disc .cue or image (default: the image in inputs/external, then ./disk)
  -o, --output PATH  file or folder to write (default: beside the input)
      --only KINDS    comma-separated kinds, for example texture,palette
      --mode MODE     raw, modern or both (default: both)
      --list          classify only, write nothing
  -q, --quiet         no summary on stdout

Examples:
  bof3 assets extract --only texture,palette -o build/art
  bof3 assets extract-all -i \"Breath of Fire III (USA).cue\" -o build/extract
  bof3 assets export texture payload.img --load-arg 0x0a081000 -o 7.png
";

const EXTRACT_HELP: &str = "\
bof3 assets extract: write the disc's assets, one file per asset.

Usage: bof3 assets extract [-i IMAGE] [-o DIR] [--only KINDS] [--mode MODE]
                             [--audio-format FMT] [--list] [--jobs N]
                             [--text-disc DIR] [-q]

  -i, --input IMAGE       CUE sheet or image (default: the one in inputs/external, then ./disk)
  -o, --output DIR        output folder (default: build/game-assets)
      --only KINDS        comma-separated kinds, for example texture,palette
                          (default: every kind)
      --mode MODE         raw writes the stored bytes, modern the conversion,
                          both writes each (default: both)
      --audio-format FMT  wav, flac or ogg (default: wav)
      --list              classify everything and write nothing (default: off)
      --jobs N            worker threads (default: every core)
      --text-disc DIR     extracted disc the text kind reads (default: build/disc)
  -q, --quiet             skip the summary on stdout (default: off)

Examples:
  bof3 assets extract --only texture,palette -o build/art
  bof3 assets extract --only audio-sequence --audio-format ogg --mode modern -o build/music
  bof3 assets extract --list --only texture
";

const COVERAGE_HELP: &str = "\
bof3 assets coverage: report which kinds rebuild their own bytes.

Usage: bof3 assets coverage [-i IMAGE]

  -i, --input IMAGE  CUE sheet or image (default: the one in inputs/external, then ./disk)

Example:
  bof3 assets coverage -i \"Breath of Fire III (USA).cue\"
";

const KINDS_HELP: &str = "\
bof3 assets kinds: list the asset kinds and what each writes.

Usage: bof3 assets kinds

Example:
  bof3 assets kinds
";

const TERRAIN_HELP: &str = "\
bof3 assets terrain: write one map bundle's terrain.

Usage: bof3 assets terrain BUNDLE [-o OUT]

  BUNDLE      stored map bundle (the area-map payload)
  -o, --output OUT
              .glb writes binary glTF; any other path writes an OBJ with its MTL
              beside it (default: BUNDLE.glb)

Example:
  bof3 assets terrain map.bin -o map.glb
";

const SOUNDFONT_HELP: &str = "\
bof3 assets soundfont: write one VAB bank as a SoundFont 2 file.

Usage: bof3 assets soundfont HEADER BODY [-o OUT]

  HEADER      the bank's .vh bytes
  BODY        the bank's .vb bytes
  -o, --output OUT
              SoundFont to write (default: HEADER.sf2)

Example:
  bof3 assets soundfont BGM001.vh BGM001.vb -o BGM001.sf2
";

const TABLES_HELP: &str = "\
bof3 assets tables: write the game tables of ETC/GAME.EMI entry 0 as JSON.

Usage: bof3 assets tables PAYLOAD [-o DIR]

  PAYLOAD     stored bytes of ETC/GAME.EMI entry 0
  -o, --output DIR
              folder for one JSON file per table (default: game-tables)

Example:
  bof3 assets tables GAME0.bin -o build/tables
";

const ENCOUNTERS_HELP: &str = "\
bof3 assets encounters: write one area's encounter block as JSON.

Usage: bof3 assets encounters PAYLOAD [-o OUT]

  PAYLOAD     stored area-encounters payload
  -o, --output OUT
              JSON to write (default: PAYLOAD.json)

Example:
  bof3 assets encounters encounters.bin -o encounters.json
";

const EXPORT_HELP: &str = "\
bof3 assets export: write one asset's open form from its stored bytes.

Usage: bof3 assets export <KIND> ...

Kinds:
  texture         PNG view, or an indexed PNG in one palette row
  palette         a CLUT bank as JSON
  terrain         a map bundle as glTF or OBJ
  vag             one sample of a VAB bank as WAVE
  sequence        one sequence of a package as MIDI
  message         a message bank as JSON
  strings         the strings of another payload as JSON
  characters      the eight playable characters as JSON
  character-bank  a sprite bank's palette tables as JSON

Every kind has focused help: bof3 assets export <KIND> --help.
Export switches: -o/--output OUT, --load-arg HEX, --depth 4|8, --palette BANK,
--palette-offset WORDS, --index N.

Example:
  bof3 assets export texture payload.img --load-arg 0x0a081000 -o 7.png
";

const IMPORT_HELP: &str = "\
bof3 assets import: read one asset's open form back into its stored bytes.

Usage: bof3 assets import <KIND> ...

Kinds:
  texture         a PNG view or indexed PNG back into a payload
  palette         palette JSON back into color words
  terrain         a glTF or OBJ mesh back into a map bundle
  vag             a WAVE file back into a sample
  table           one game table back into ETC/GAME.EMI entry 0
  encounters      encounter JSON back into its block
  layout          map layout JSON back into a bundle
  emi             a container with named entries replaced
  sequence        MIDI files back into a sequence package
  disc            a patched copy of the whole disc
  soundfont       a SoundFont 2 file back into a VAB bank
  message         message JSON back into a bank
  strings         strings JSON back into its payload
  characters      character JSON back into ETC/START.EMI entry 8
  character-bank  palette JSON back into a sprite bank

Every kind has focused help: bof3 assets import <KIND> --help.
Import switches: -o/--output OUT, --payload PATH, --palette OUT, --base BLOCK,
--entry INDEX=FILE, --file PATH=FILE.

Example:
  bof3 assets import texture 7.png -o payload.img
";

const EXPORT_TEXTURE_HELP: &str = "\
bof3 assets export texture: write a texture's stored payload as a PNG.

Usage: bof3 assets export texture PAYLOAD --load-arg HEX [-o OUT]
       bof3 assets export texture PAYLOAD --load-arg HEX --depth 4|8 --palette BANK
                                           [--palette-offset WORDS] [-o OUT]

  PAYLOAD                  stored texture payload
      --load-arg HEX       the entry's load address (required)
      --depth 4|8          draw an indexed PNG at this depth (default: off, 16 bpp view)
      --palette BANK       color-word file for --depth (required with it)
      --palette-offset N   first word of the palette row (default: 0)
  -o, --output OUT         PNG to write (default: PAYLOAD.png)

Examples:
  bof3 assets export texture payload.img --load-arg 0x0a081000 -o 7.png
  bof3 assets export texture payload.img --load-arg 0x0a081000 --depth 4 --palette bank.clut -o 7.png
";

const EXPORT_TERRAIN_HELP: &str = "\
bof3 assets export terrain: write one map bundle's terrain.

Usage: bof3 assets export terrain BUNDLE [-o OUT]

  BUNDLE              stored map bundle (the area-map payload)
  -o, --output OUT    .glb writes binary glTF; any other path writes an OBJ with
                      its MTL beside it (default: BUNDLE.glb)

Example:
  bof3 assets export terrain map.bin -o map.glb
";

const EXPORT_PALETTE_HELP: &str = "\
bof3 assets export palette: write a CLUT bank as JSON.

Usage: bof3 assets export palette BANK [-o OUT]

  BANK                raw color words
  -o, --output OUT    JSON to write (default: BANK.json)

Example:
  bof3 assets export palette palette.clut -o palette.json
";

const EXPORT_VAG_HELP: &str = "\
bof3 assets export vag: write one sample of a VAB bank as a WAVE file.

Usage: bof3 assets export vag HEADER BODY --index N [-o OUT]

  HEADER              the bank's .vh bytes
  BODY                the bank's .vb bytes
      --index N       sample number, 1-based (required)
  -o, --output OUT    WAVE to write (default: HEADER-N.wav)

Example:
  bof3 assets export vag BGM001.vh BGM001.vb --index 1 -o sample.wav
";

const EXPORT_SEQUENCE_HELP: &str = "\
bof3 assets export sequence: write one sequence of a package as MIDI.

Usage: bof3 assets export sequence PACKAGE [--index N] [-o OUT]

  PACKAGE             sequence package (.seq payload)
      --index N       sequence to write (default: 0)
  -o, --output OUT    MIDI to write (default: PACKAGE.mid)

Example:
  bof3 assets export sequence package.seq --index 1 -o song.mid
";

const EXPORT_MESSAGE_HELP: &str = "\
bof3 assets export message: write a message bank as JSON.

Usage: bof3 assets export message PAYLOAD [-o OUT]

  PAYLOAD             message-bank payload
  -o, --output OUT    JSON to write (default: PAYLOAD.json)

Example:
  bof3 assets export message bank.bin -o bank.json
";

const EXPORT_STRINGS_HELP: &str = "\
bof3 assets export strings: write the strings of a payload as JSON.

Usage: bof3 assets export strings PAYLOAD [--load-arg HEX] [-o OUT]

  PAYLOAD             payload that holds strings
      --load-arg HEX  the entry's load address, for type 0-2 payloads
                      (default: none, so a pointer table is not recognised)
  -o, --output OUT    JSON to write (default: PAYLOAD.json)

Example:
  bof3 assets export strings payload.bin --load-arg 0x80100000 -o strings.json
";

const EXPORT_CHARACTERS_HELP: &str = "\
bof3 assets export characters: write the eight playable characters as JSON.

Usage: bof3 assets export characters PAYLOAD [-o OUT]

  PAYLOAD             ETC/START.EMI entry 8
  -o, --output OUT    JSON to write (default: PAYLOAD.json)

Example:
  bof3 assets export characters START8.bin -o characters.json
";

const EXPORT_CHARACTER_BANK_HELP: &str = "\
bof3 assets export character-bank: write a sprite bank's palettes as JSON.

Usage: bof3 assets export character-bank PAYLOAD [-o OUT]

  PAYLOAD             decoded sprite bank, container or one section
  -o, --output OUT    JSON to write (default: PAYLOAD.json)

Example:
  bof3 assets export character-bank bank.bin -o palettes.json
";

const IMPORT_TEXTURE_HELP: &str = "\
bof3 assets import texture: read a texture PNG back into its stored payload.

Usage: bof3 assets import texture PNG [-o OUT] [--palette OUT]

  PNG                 a PNG that export texture wrote
  -o, --output OUT    payload to write (default: PNG.img)
      --palette OUT   also write an indexed PNG's color words

Examples:
  bof3 assets import texture 7.png -o payload.img
  bof3 assets import texture 7.png --palette bank.clut -o payload.img
";

const IMPORT_PALETTE_HELP: &str = "\
bof3 assets import palette: read palette JSON back into color words.

Usage: bof3 assets import palette JSON [-o OUT]

  JSON                palette JSON from export palette
  -o, --output OUT    raw color words to write (default: JSON.bin)

Example:
  bof3 assets import palette palette.json -o palette.clut
";

const IMPORT_TERRAIN_HELP: &str = "\
bof3 assets import terrain: read a mesh back into its map bundle.

Usage: bof3 assets import terrain MESH --payload BUNDLE [-o OUT]

  MESH                .glb or OBJ mesh from export terrain
      --payload BUNDLE
                      the stored bundle the mesh came from; supplies every
                      other section (required)
  -o, --output OUT    bundle to write (default: MESH.bin)

Example:
  bof3 assets import terrain map.glb --payload map.bin -o rebuilt.bin
";

const IMPORT_VAG_HELP: &str = "\
bof3 assets import vag: read a WAVE file back into a sample.

Usage: bof3 assets import vag WAV [-o OUT]

  WAV                 44.1 kHz mono 16-bit WAVE
  -o, --output OUT    VAG to write (default: WAV.vag)

Example:
  bof3 assets import vag sample.wav -o sample.vag
";

const IMPORT_TABLE_HELP: &str = "\
bof3 assets import table: read one game table back into GAME.EMI entry 0.

Usage: bof3 assets import table NAME JSON --payload ENTRY [-o OUT]

  NAME                table name, as its JSON file is named
  JSON                table JSON from `assets tables`
      --payload ENTRY the stored entry 0 (required); bytes the fields do not
                      name keep their values from it
  -o, --output OUT    entry to write (default: JSON.bin)

Example:
  bof3 assets import table items items.json --payload GAME0.bin -o GAME0.new.bin
";

const IMPORT_ENCOUNTERS_HELP: &str = "\
bof3 assets import encounters: read encounter JSON back into its block.

Usage: bof3 assets import encounters JSON --base BLOCK [-o OUT]

  JSON                encounter JSON from `assets encounters`
      --base BLOCK    the stored block (required); unnamed bytes are kept
  -o, --output OUT    block to write (default: JSON.bin)

Example:
  bof3 assets import encounters encounters.json --base encounters.bin -o rebuilt.bin
";

const IMPORT_LAYOUT_HELP: &str = "\
bof3 assets import layout: read map layout JSON back into a bundle.

Usage: bof3 assets import layout JSON --payload BUNDLE [-o OUT]

  JSON                layout JSON from `assets extract`
      --payload BUNDLE
                      the stored bundle (required); supplies every other section
  -o, --output OUT    bundle to write (default: JSON.bin)

Example:
  bof3 assets import layout map.layout.json --payload map.bin -o rebuilt.bin
";

const IMPORT_EMI_HELP: &str = "\
bof3 assets import emi: rebuild a container with named entries replaced.

Usage: bof3 assets import emi CONTAINER [--entry INDEX=FILE]... [-o OUT]

  CONTAINER           the original .EMI bytes
      --entry INDEX=FILE
                      replace entry INDEX with FILE (repeatable)
  -o, --output OUT    archive to write (default: CONTAINER.rebuilt.EMI)

Example:
  bof3 assets import emi BGM000.EMI --entry 2=new.bin -o BGM000.new.EMI
";

const IMPORT_SEQUENCE_HELP: &str = "\
bof3 assets import sequence: write a sequence package from MIDI files.

Usage: bof3 assets import sequence FILE.mid:ID... [-o OUT]

  FILE.mid:ID         one file per sequence, in order; ID is its package id
  -o, --output OUT    package to write (default: first file with .seq)

Example:
  bof3 assets import sequence song.mid:1 other.mid:2 -o package.seq
";

const IMPORT_DISC_HELP: &str = "\
bof3 assets import disc: write a patched copy of the whole disc.

Usage: bof3 assets import disc CUE [--file PATH=FILE]... -o DIR

  CUE                 the disc's cue sheet
      --file PATH=FILE
                      replace disc file PATH with FILE (repeatable); a file
                      cannot grow past its last sector
  -o, --output DIR    folder for the patched disc (required)

Example:
  bof3 assets import disc game.cue --file BIN/ETC/GAME.EMI=GAME.EMI -o build/patched
";

const IMPORT_SOUNDFONT_HELP: &str = "\
bof3 assets import soundfont: read a SoundFont 2 file back into a VAB bank.

Usage: bof3 assets import soundfont BANK.sf2 [-o PREFIX]

  BANK.sf2            the SoundFont
  -o, --output PREFIX writes PREFIX.vh and PREFIX.vb (default: BANK)

Example:
  bof3 assets import soundfont BGM001.sf2 -o BGM001
";

const IMPORT_MESSAGE_HELP: &str = "\
bof3 assets import message: read message JSON back into a bank.

Usage: bof3 assets import message JSON [-o OUT]

  JSON                message JSON from export message
  -o, --output OUT    bank to write (default: JSON.bin)

Example:
  bof3 assets import message bank.json -o bank.bin
";

const IMPORT_STRINGS_HELP: &str = "\
bof3 assets import strings: read strings JSON back into its payload.

Usage: bof3 assets import strings JSON --payload PAYLOAD [-o OUT]

  JSON                strings JSON from export strings
      --payload PATH  the payload the JSON came from (required)
  -o, --output OUT    payload to write (default: JSON.bin)

Example:
  bof3 assets import strings strings.json --payload payload.bin -o rebuilt.bin
";

const IMPORT_CHARACTERS_HELP: &str = "\
bof3 assets import characters: read character JSON back into START.EMI entry 8.

Usage: bof3 assets import characters JSON --payload PAYLOAD [-o OUT]

  JSON                character JSON from export characters
      --payload PATH  the stored entry 8 (required)
  -o, --output OUT    entry to write (default: JSON.bin)

Example:
  bof3 assets import characters characters.json --payload START8.bin -o START8.new.bin
";

const IMPORT_CHARACTER_BANK_HELP: &str = "\
bof3 assets import character-bank: read palette JSON back into a sprite bank.

Usage: bof3 assets import character-bank JSON --payload PAYLOAD [-o OUT]

  JSON                palette JSON from export character-bank
      --payload PATH  the decoded bank (required)
  -o, --output OUT    bank to write (default: JSON.bin)

Example:
  bof3 assets import character-bank palettes.json --payload bank.bin -o bank.new.bin
";

fn export_help(kind: &str) -> &'static str {
    match kind {
        "texture" => EXPORT_TEXTURE_HELP,
        "palette" => EXPORT_PALETTE_HELP,
        "terrain" => EXPORT_TERRAIN_HELP,
        "vag" => EXPORT_VAG_HELP,
        "sequence" => EXPORT_SEQUENCE_HELP,
        "message" => EXPORT_MESSAGE_HELP,
        "strings" => EXPORT_STRINGS_HELP,
        "characters" => EXPORT_CHARACTERS_HELP,
        "character-bank" => EXPORT_CHARACTER_BANK_HELP,
        _ => EXPORT_HELP,
    }
}

fn import_help(kind: &str) -> &'static str {
    match kind {
        "texture" => IMPORT_TEXTURE_HELP,
        "palette" => IMPORT_PALETTE_HELP,
        "terrain" => IMPORT_TERRAIN_HELP,
        "vag" => IMPORT_VAG_HELP,
        "table" => IMPORT_TABLE_HELP,
        "encounters" => IMPORT_ENCOUNTERS_HELP,
        "layout" => IMPORT_LAYOUT_HELP,
        "emi" => IMPORT_EMI_HELP,
        "sequence" => IMPORT_SEQUENCE_HELP,
        "disc" => IMPORT_DISC_HELP,
        "soundfont" => IMPORT_SOUNDFONT_HELP,
        "message" => IMPORT_MESSAGE_HELP,
        "strings" => IMPORT_STRINGS_HELP,
        "characters" => IMPORT_CHARACTERS_HELP,
        "character-bank" => IMPORT_CHARACTER_BANK_HELP,
        _ => IMPORT_HELP,
    }
}

pub fn main(args: Vec<OsString>) -> ExitCode {
    match run(args) {
        Ok(code) => ExitCode::from(code as u8),
        Err(error) => {
            eprintln!("bof3 assets: {error}");
            ExitCode::from(EXIT_ERROR as u8)
        }
    }
}

fn run(args: Vec<OsString>) -> Result<i32> {
    let args: Vec<String> = args.into_iter().map(|arg| arg.to_string_lossy().into_owned()).collect();
    let Some(first) = args.first() else {
        print!("{USAGE}");
        return Ok(EXIT_OK);
    };
    if matches!(first.as_str(), "-h" | "--help" | "help") {
        print!("{USAGE}");
        return Ok(EXIT_OK);
    }
    let rest = &args[1..];
    if rest.iter().any(|arg| matches!(arg.as_str(), "-h" | "--help" | "help")) {
        print!("{}", help_for(&args));
        return Ok(EXIT_OK);
    }
    let rest: Vec<String> = rest.to_vec();
    let help = help_hint(first, &rest);
    let result = match first.as_str() {
        "kinds" => {
            for kind in Kind::ALL {
                println!("{:<16} {:<9} {}", kind.name(), kind.handling().label(), kind.about());
            }
            Ok(EXIT_OK)
        }
        "extract" => extract_command(&rest),
        "extract-all" => crate::extract_all::run(&rest).map_err(Error::Usage),
        "coverage" => coverage_command(&rest),
        "terrain" => terrain_command(&rest),
        "soundfont" => soundfont_command(&rest),
        "tables" => tables_command(&rest),
        "encounters" => encounters_command(&rest),
        "export" => convert_command(Direction::Export, &rest),
        "import" => convert_command(Direction::Import, &rest),
        other => Err(Error::Usage(format!("unknown command {other:?}; run `bof3 assets --help` for the commands"))),
    };
    result.map_err(|error| {
        let message = error.to_string();
        if message.contains("--help") { error } else { Error::Usage(format!("{message}; run `{help} --help`")) }
    })
}

/// The focused help command for a verb and its first argument.
fn help_hint(verb: &str, rest: &[String]) -> String {
    if matches!(verb, "export" | "import")
        && let Some(kind) = rest.iter().find(|argument| !argument.starts_with('-'))
    {
        return format!("bof3 assets {verb} {kind}");
    }
    format!("bof3 assets {verb}")
}

/// The focused help for the command in `args`: a verb, or an export/import
/// sub-verb.
fn help_for(args: &[String]) -> String {
    let Some(verb) = args.first() else {
        return USAGE.to_string();
    };
    let sub = args.get(1).map(String::as_str);
    match (verb.as_str(), sub) {
        ("extract", _) => EXTRACT_HELP.to_string(),
        ("extract-all", _) => crate::extract_all::USAGE.to_string(),
        ("coverage", _) => COVERAGE_HELP.to_string(),
        ("terrain", _) => TERRAIN_HELP.to_string(),
        ("soundfont", _) => SOUNDFONT_HELP.to_string(),
        ("tables", _) => TABLES_HELP.to_string(),
        ("encounters", _) => ENCOUNTERS_HELP.to_string(),
        ("kinds", _) => KINDS_HELP.to_string(),
        ("export", Some(kind)) => export_help(kind).to_string(),
        ("import", Some(kind)) => import_help(kind).to_string(),
        ("export", None) => EXPORT_HELP.to_string(),
        ("import", None) => IMPORT_HELP.to_string(),
        _ => USAGE.to_string(),
    }
}

/// Parses `args` into `T`. The help text is printed by `run`, so clap's own help is off; a parse
/// error becomes a one-line usage error.
fn parse<T: Parser>(args: &[String]) -> Result<T> {
    let command = T::command()
        .no_binary_name(true)
        .disable_help_flag(true)
        .disable_version_flag(true)
        .disable_help_subcommand(true)
        .args_override_self(true);
    let usage = |error: clap::Error| {
        let text = error.to_string();
        let line = text.lines().next().unwrap_or_default();
        Error::Usage(line.strip_prefix("error: ").unwrap_or(line).to_string())
    };
    let matches = command.try_get_matches_from(args).map_err(usage)?;
    T::from_arg_matches(&matches).map_err(usage)
}

/// The one file a command takes, or `missing` if there is none.
fn single(mut files: Vec<PathBuf>, missing: &str) -> Result<PathBuf> {
    if files.len() > 1 {
        return Err(Error::Usage(format!("unexpected argument {:?}", files[1].to_string_lossy())));
    }
    files.pop().ok_or_else(|| Error::Usage(missing.into()))
}

/// The `-o` option every conversion command takes.
fn with_extension_of(out: Option<PathBuf>, input: &Path, extension: &str) -> PathBuf {
    out.unwrap_or_else(|| input.with_extension(extension))
}

/// Reads a number written in decimal or with a `0x` prefix.
fn number(text: &str, flag: &str) -> Result<u32> {
    let parsed = match text.strip_prefix("0x") {
        Some(hex) => u32::from_str_radix(hex, 16),
        None => text.parse(),
    };
    parsed.map_err(|_| Error::Usage(format!("{flag} needs a number, not {text:?}")))
}

fn optional_number(text: Option<&str>, flag: &str) -> Result<Option<u32>> {
    text.map(|text| number(text, flag)).transpose()
}

/// Reads a file and names it when the read fails.
fn read_file(path: impl AsRef<Path>) -> Result<Vec<u8>> {
    let path = path.as_ref();
    fs::read(path).map_err(|error| Error::Usage(format!("cannot read {}: {error}", path.display())))
}

/// Creates the directory that holds `out`, if it has one.
fn create_parent(out: &Path) -> Result<()> {
    if let Some(parent) = out.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

/// Writes `bytes` to `out`, creating its directory, and reports the file.
fn write_output(out: &Path, bytes: &[u8]) -> Result<()> {
    create_parent(out)?;
    fs::write(out, bytes)?;
    println!("wrote {}", out.display());
    Ok(())
}

/// The little-endian color words of a palette file.
fn color_words(bytes: &[u8]) -> Result<Vec<u16>> {
    if !bytes.len().is_multiple_of(2) {
        return Err(Error::Usage("a palette file is whole 16-bit color words".into()));
    }
    Ok(bytes.as_chunks::<2>().0.iter().map(|&pair| u16::from_le_bytes(pair)).collect())
}

/// Splits `INDEX=FILE` or `PATH=FILE` for `flag`.
fn pair<'a>(value: &'a str, flag: &str, shape: &str) -> Result<(&'a str, &'a str)> {
    value.split_once('=').ok_or_else(|| Error::Usage(format!("{flag} needs {shape}, not {value:?}")))
}

/// The arguments of the table-driven conversions (`export` and `import` of a kind in
/// [`codec::CODECS`]): files, `-o`, and the options an op may take.
#[derive(Parser)]
struct Convert {
    files: Vec<String>,
    #[arg(short = 'o', long = "output", alias = "out")]
    output: Option<PathBuf>,
    #[arg(long)]
    payload: Option<PathBuf>,
    #[arg(long)]
    base: Option<PathBuf>,
    #[arg(long)]
    index: Option<String>,
    #[arg(long = "load-arg")]
    load_arg: Option<String>,
}

/// `bof3 assets export|import KIND ...`: the verbs that convert one asset to or from its open form.
fn convert_command(direction: Direction, args: &[String]) -> Result<i32> {
    let verb = if direction == Direction::Export { "export" } else { "import" };
    let Some((kind, rest)) = args.split_first() else {
        return Err(Error::Usage(format!("{verb} needs a kind, for example texture; run `bof3 assets {verb} --help`")));
    };
    match (direction, kind.as_str()) {
        (Direction::Export, "texture") => return export_texture(rest),
        (Direction::Export, "terrain") => return terrain_command(rest),
        (Direction::Import, "texture") => return import_texture(rest),
        (Direction::Import, "emi") => return import_emi(rest),
        (Direction::Import, "sequence") => return import_sequence(rest),
        (Direction::Import, "disc") => return import_disc(rest),
        (Direction::Import, "soundfont") => return import_soundfont(rest),
        _ => {}
    }
    let Some(op) = codec::find(kind).and_then(|codec| codec.op(direction)) else {
        return Err(Error::Usage(match direction {
            Direction::Export => format!(
                "export has no {kind:?} direction; run `bof3 assets export --help` for texture, palette, terrain, vag, sequence, message, strings, characters and character-bank"
            ),
            Direction::Import => {
                format!("import has no {kind:?} direction; run `bof3 assets import --help` for the supported kinds")
            }
        }));
    };
    let args: Convert = parse(rest)?;
    reject_unused_options(op, &args)?;
    let wanted = op.files + usize::from(op.named);
    if args.files.len() < wanted {
        return Err(Error::Usage(op.needs_input.into()));
    }
    if let Some(extra) = args.files.get(wanted) {
        return Err(Error::Usage(format!("unexpected argument {extra:?}")));
    }
    let index = optional_number(args.index.as_deref(), "--index")?;
    if let Index::Required(missing) = op.index
        && index.is_none()
    {
        return Err(Error::Usage(missing.into()));
    }
    let flagged = match op.companion_flag {
        Some(flag) => {
            let path = if flag.flag == "--base" { &args.base } else { &args.payload };
            Some(path.as_ref().ok_or_else(|| Error::Usage(flag.missing.into()))?)
        }
        None => None,
    };
    let (name, files) =
        if op.named { (args.files.first().map(String::as_str), &args.files[1..]) } else { (None, &args.files[..]) };
    let input_path = Path::new(&files[0]);
    let input = read_file(input_path)?;
    let second = match (op.files, flagged) {
        (2, _) => Some(read_file(&files[1])?),
        (_, Some(path)) => Some(read_file(path)?),
        _ => None,
    };
    let job = Job {
        name,
        input: &input,
        companion: second.as_deref(),
        index,
        load_arg: optional_number(args.load_arg.as_deref(), "--load-arg")?,
    };
    let bytes = (op.run)(&job)?;
    write_output(&with_extension_of(args.output, input_path, &op.extension_for(index)), &bytes)?;
    Ok(EXIT_OK)
}

/// An option the op does not take is an unexpected argument, as it was before the table.
fn reject_unused_options(op: &codec::Op, args: &Convert) -> Result<()> {
    let flag = op.companion_flag.map(|flag| flag.flag);
    let unused = [
        ("--payload", args.payload.is_some() && flag != Some("--payload")),
        ("--base", args.base.is_some() && flag != Some("--base")),
        ("--index", args.index.is_some() && op.index == Index::No),
        ("--load-arg", args.load_arg.is_some() && !op.load_arg),
    ];
    match unused.into_iter().find(|(_, used)| *used) {
        Some((name, _)) => Err(Error::Usage(format!("unexpected argument {name:?}"))),
        None => Ok(()),
    }
}

/// `bof3 assets export texture PAYLOAD --load-arg HEX [--depth 4|8 --palette BANK [--palette-offset WORDS]] [-o OUT]`:
/// writes a texture's stored payload as a PNG. Without `--depth` the PNG is the 16 bpp view. With it, the
/// payload is drawn with the palette row that starts at word WORDS of BANK, a file of raw color words.
#[derive(Parser)]
struct ExportTexture {
    files: Vec<PathBuf>,
    #[arg(long = "load-arg")]
    load_arg: Option<String>,
    #[arg(long)]
    depth: Option<String>,
    #[arg(long)]
    palette: Option<PathBuf>,
    #[arg(long = "palette-offset")]
    palette_offset: Option<String>,
    #[arg(short = 'o', long = "output", alias = "out")]
    output: Option<PathBuf>,
}

fn export_texture(args: &[String]) -> Result<i32> {
    let args: ExportTexture = parse(args)?;
    let load_arg = optional_number(args.load_arg.as_deref(), "--load-arg")?;
    let depth = optional_number(args.depth.as_deref(), "--depth")?;
    let offset = optional_number(args.palette_offset.as_deref(), "--palette-offset")?.unwrap_or(0) as usize;
    let payload_path = single(args.files, "export texture needs a payload file")?;
    let load_arg =
        load_arg.ok_or_else(|| Error::Usage("export texture needs --load-arg, the entry's load address".into()))?;
    let bytes = read_file(&payload_path)?;
    let (png, what) = match depth {
        None => (decode::texture_view_png(load_arg, &bytes)?, "16 bpp view".to_string()),
        Some(bits) => {
            let Some(bank) = args.palette else {
                return Err(Error::Usage("--depth needs --palette, the bank that holds the row".into()));
            };
            let count = match bits {
                4 => 16,
                8 => 256,
                other => return Err(Error::Usage(format!("--depth is 4 or 8, not {other}"))),
            };
            let words = color_words(&read_file(&bank)?)?;
            let row = words
                .get(offset..offset + count)
                .ok_or_else(|| Error::Usage(format!("{} has no {count}-color row at word {offset}", bank.display())))?;
            let png = decode::texture_row_png(load_arg, &bytes, bits, row)?;
            (png, format!("{bits}-bit indexed row at word {offset}"))
        }
    };
    let out = with_extension_of(args.output, &payload_path, "png");
    create_parent(&out)?;
    fs::write(&out, png)?;
    println!("wrote {} ({what})", out.display());
    Ok(EXIT_OK)
}

/// `bof3 assets import texture PNG [-o OUT] [--palette OUT]`: reads a texture PNG that `export` wrote back
/// into its stored payload (default: the PNG with a `.img` extension). An indexed PNG also has color words,
/// which `--palette` writes as raw little-endian words.
#[derive(Parser)]
struct ImportTexture {
    files: Vec<PathBuf>,
    #[arg(short = 'o', long = "output", alias = "out")]
    output: Option<PathBuf>,
    #[arg(long)]
    palette: Option<PathBuf>,
}

fn import_texture(args: &[String]) -> Result<i32> {
    let args: ImportTexture = parse(args)?;
    let input = single(args.files, "import texture needs a PNG file")?;
    let out = with_extension_of(args.output, &input, "img");
    create_parent(&out)?;
    match import::texture(&read_file(&input)?)? {
        TextureImport::Words(payload) => {
            fs::write(&out, payload)?;
            println!("wrote {} (16 bpp view)", out.display());
        }
        TextureImport::Indexed(texture) => {
            fs::write(&out, &texture.payload)?;
            println!("wrote {} ({}-bit indexed row)", out.display(), texture.depth.bits());
            if let Some(path) = args.palette {
                let bytes: Vec<u8> = texture.palette.iter().flat_map(|word| word.to_le_bytes()).collect();
                fs::write(&path, bytes)?;
                println!("wrote {} ({} color words)", path.display(), texture.palette.len());
            }
        }
    }
    Ok(EXIT_OK)
}

/// `bof3 assets import sequence FILE.mid:ID... [-o OUT]`: writes a sequence package (`.seq`) from Standard MIDI
/// Files, one sequence per argument and in order. ID is the sequence's id in the package. The output defaults to
/// the first file's name with a `.seq` extension.
#[derive(Parser)]
struct ImportSequence {
    parts: Vec<String>,
    #[arg(short = 'o', long = "output", alias = "out")]
    output: Option<PathBuf>,
}

fn import_sequence(args: &[String]) -> Result<i32> {
    let args: ImportSequence = parse(args)?;
    let parts = args
        .parts
        .iter()
        .map(|part| {
            let (path, id) = part.rsplit_once(':').ok_or_else(|| Error::Usage(format!("{part:?} needs FILE:ID")))?;
            let id: u16 = id.parse().map_err(|_| Error::Usage(format!("{part:?} needs a numeric ID")))?;
            Ok((PathBuf::from(path), id))
        })
        .collect::<Result<Vec<_>>>()?;
    let Some((first, _)) = parts.first() else {
        return Err(Error::Usage("import sequence needs at least one FILE:ID".into()));
    };
    let default = first.with_extension("seq");
    let files = parts
        .iter()
        .map(|(path, id)| -> Result<(Vec<u8>, u16)> { Ok((read_file(path)?, *id)) })
        .collect::<Result<Vec<_>>>()?;
    let borrowed: Vec<(&[u8], u16)> = files.iter().map(|(bytes, id)| (bytes.as_slice(), *id)).collect();
    write_output(&args.output.unwrap_or(default), &rebof3_formats::sep::midi::import(&borrowed)?)?;
    Ok(EXIT_OK)
}

/// `bof3 assets import emi CONTAINER [--entry INDEX=FILE]... [-o OUT]`: rebuilds an EMI container with the entries
/// named by `--entry` replaced by the files' bytes. Without `--entry` the output is the container as its entries
/// pack. The output defaults to CONTAINER with a `.rebuilt.EMI` extension.
#[derive(Parser)]
struct ImportEmi {
    files: Vec<PathBuf>,
    #[arg(long)]
    entry: Vec<String>,
    #[arg(short = 'o', long = "output", alias = "out")]
    output: Option<PathBuf>,
}

fn import_emi(args: &[String]) -> Result<i32> {
    let args: ImportEmi = parse(args)?;
    let replacements = args
        .entry
        .iter()
        .map(|value| {
            let (index, file) = pair(value, "--entry", "INDEX=FILE")?;
            Ok((number(index, "--entry")? as usize, PathBuf::from(file)))
        })
        .collect::<Result<Vec<_>>>()?;
    let container = single(args.files, "import emi needs the container file")?;
    let payloads = replacements
        .iter()
        .map(|(index, file)| -> Result<(usize, Vec<u8>)> { Ok((*index, read_file(file)?)) })
        .collect::<Result<Vec<_>>>()?;
    let replace: Vec<(usize, &[u8])> = payloads.iter().map(|(index, bytes)| (*index, bytes.as_slice())).collect();
    let bytes = import::emi(&read_file(&container)?, &replace)?;
    write_output(&with_extension_of(args.output, &container, "rebuilt.EMI"), &bytes)?;
    Ok(EXIT_OK)
}

/// `bof3 assets import disc CUE [--file PATH=FILE]... -o DIR`: writes a patched copy of the disc into the new directory
/// DIR. Each `--file` replaces the disc file PATH with the bytes of FILE, keeping its sectors. The copy keeps the cue
/// sheet and every track file under its own name.
#[derive(Parser)]
struct ImportDisc {
    files: Vec<PathBuf>,
    #[arg(long)]
    file: Vec<String>,
    #[arg(short = 'o', long = "output", alias = "out")]
    output: Option<PathBuf>,
}

fn import_disc(args: &[String]) -> Result<i32> {
    let args: ImportDisc = parse(args)?;
    let replacements = args
        .file
        .iter()
        .map(|value| {
            let (path, file) = pair(value, "--file", "PATH=FILE")?;
            Ok((PathBuf::from(path), PathBuf::from(file)))
        })
        .collect::<Result<Vec<_>>>()?;
    let cue = single(args.files, "import disc needs the cue sheet of the disc")?;
    let out = args
        .output
        .ok_or_else(|| Error::Usage("import disc needs -o DIR, a new directory for the patched disc".into()))?;
    let replacements = replacements
        .into_iter()
        .map(|(path, file)| -> Result<disc::Replacement> { Ok(disc::Replacement { path, bytes: read_file(file)? }) })
        .collect::<Result<Vec<_>>>()?;
    let summary = disc::patch(&cue, &out, &replacements)?;
    println!("patched {} files and sealed {} sectors in {}", summary.files, summary.sealed, out.display());
    Ok(EXIT_OK)
}

/// `bof3 assets import soundfont BANK.sf2 [-o PREFIX]`: writes the VAB bank a SoundFont 2 file describes as
/// PREFIX.vh and PREFIX.vb. The default prefix is the SoundFont's path without its extension.
#[derive(Parser)]
struct OneFile {
    files: Vec<PathBuf>,
    #[arg(short = 'o', long = "output", alias = "out")]
    output: Option<PathBuf>,
}

fn import_soundfont(args: &[String]) -> Result<i32> {
    let args: OneFile = parse(args)?;
    let input = single(args.files, "import soundfont needs a SoundFont 2 file")?;
    let (header, body) = soundfont::bank(&read_file(&input)?)?;
    let prefix = args.output.unwrap_or_else(|| input.with_extension(""));
    write_output(&PathBuf::from(format!("{}.vh", prefix.display())), &header)?;
    write_output(&PathBuf::from(format!("{}.vb", prefix.display())), &body)?;
    Ok(EXIT_OK)
}

/// `bof3 assets soundfont HEADER BODY [-o OUT]`: writes one VAB bank as a SoundFont 2 file. HEADER is
/// the `.vh` and BODY the `.vb`. The output defaults to HEADER with a `.sf2` extension.
fn soundfont_command(args: &[String]) -> Result<i32> {
    let args: OneFile = parse(args)?;
    let [header, body] = args.files.as_slice() else {
        return Err(Error::Usage("soundfont needs a header (.vh) and a body (.vb) file".into()));
    };
    let out = with_extension_of(args.output, header, "sf2");
    create_parent(&out)?;
    let title = out.file_stem().unwrap_or_default().to_string_lossy().into_owned();
    let data = decode::soundfont(&read_file(header)?, &read_file(body)?, &title)?;
    fs::write(&out, data)?;
    println!("wrote {}", out.display());
    Ok(EXIT_OK)
}

/// `bof3 assets terrain BUNDLE [-o OUT]`: writes one map bundle's terrain. A `.glb` output is a
/// binary glTF. Any other output is an OBJ, and its MTL goes beside it.
fn terrain_command(args: &[String]) -> Result<i32> {
    let args: OneFile = parse(args)?;
    let bundle = single(args.files, "terrain needs a map bundle file")?;
    let out = with_extension_of(args.output, &bundle, "glb");
    create_parent(&out)?;
    let mesh = terrain::mesh_from_payload(&read_file(&bundle)?)?;
    let stem = out.file_stem().unwrap_or_default().to_string_lossy().into_owned();
    if out.extension().is_some_and(|extension| extension == "glb") {
        let glb = terrain::glb_of_mesh(&mesh, &stem)
            .ok_or_else(|| Error::Usage(format!("{} has no cell to draw", bundle.display())))?;
        fs::write(&out, glb)?;
    } else {
        let mtl = out.with_extension("mtl");
        let mtl_name = mtl.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let text = terrain::of_mesh(&mesh, &mtl_name);
        fs::write(&out, text.obj)?;
        fs::write(&mtl, text.mtl)?;
    }
    println!("wrote {}", out.display());
    Ok(EXIT_OK)
}

/// `bof3 assets encounters PAYLOAD [-o OUT]`: writes one area's encounter block as JSON. PAYLOAD is the
/// stored bytes of an `area-encounters` entry. The output defaults to PAYLOAD with a `.json` extension.
fn encounters_command(args: &[String]) -> Result<i32> {
    let args: OneFile = parse(args)?;
    let payload = single(args.files, "encounters needs an area encounter block file")?;
    let out = with_extension_of(args.output, &payload, "json");
    create_parent(&out)?;
    fs::write(&out, rebof3_formats::encounter::open::export(&read_file(&payload)?)?)?;
    println!("wrote {}", out.display());
    Ok(EXIT_OK)
}

/// `bof3 assets tables PAYLOAD [-o DIR]`: writes the fixed game tables of `ETC/GAME.EMI` entry 0 as one
/// JSON file per table. PAYLOAD is the stored bytes of that entry.
fn tables_command(args: &[String]) -> Result<i32> {
    let args: OneFile = parse(args)?;
    let payload = single(args.files, "tables needs the stored bytes of GAME.EMI entry 0")?;
    let out = args.output.unwrap_or_else(|| PathBuf::from("game-tables"));
    let files = rebof3_formats::game_tables::open::files(&read_file(&payload)?)?;
    fs::create_dir_all(&out)?;
    for (name, data) in &files {
        fs::write(out.join(format!("{name}.json")), data)?;
    }
    println!("wrote {} tables to {}", files.len(), out.display());
    Ok(EXIT_OK)
}

#[derive(Parser)]
struct CoverageArgs {
    #[arg(short = 'i', long = "input", alias = "image")]
    image: Option<PathBuf>,
}

/// `bof3 assets coverage [-i IMAGE]`: reads every EMI entry of the disc and prints, per kind, the entries and bytes that
/// export and import to their own bytes, against the kind's total. Kinds without a round trip report zero.
fn coverage_command(args: &[String]) -> Result<i32> {
    let image = parse::<CoverageArgs>(args)?.image;
    let current = env::current_dir()?;
    let image = match image {
        Some(image) => image,
        None => find_disc_image(&current).map_err(Error::Usage)?,
    };
    let report = coverage::run(&image)?;
    println!("{:<16} {:>8} {:>10} {:>13} {:>13}  note", "kind", "entries", "both ways", "bytes", "both ways");
    let (mut entries, mut bytes, mut both_entries, mut both_bytes) = (0, 0, 0, 0);
    for kind in Kind::ALL {
        let Some(line) = report.kinds.get(kind) else { continue };
        if *kind != Kind::DataTable {
            entries += line.entries;
            bytes += line.bytes;
            both_entries += line.both_entries;
            both_bytes += line.both_bytes;
        }
        let note = if coverage::has_round_trip(*kind) { "" } else { "no round trip yet" };
        let note = if *kind == Kind::DataTable {
            "game tables of ETC/GAME.EMI entry 0 and START.EMI's characters"
        } else {
            note
        };
        println!(
            "{:<16} {:>8} {:>10} {:>13} {:>13}  {note}",
            kind.name(),
            line.entries,
            line.both_entries,
            line.bytes,
            line.both_bytes
        );
    }
    let text = report.text;
    println!(
        "{:<16} {:>8} {:>10} {:>13} {:>13}  strings and message banks",
        "text", text.entries, text.both_entries, text.bytes, text.both_bytes
    );
    let palettes = report.palettes;
    println!(
        "{:<16} {:>8} {:>10} {:>13} {:>13}  palette tables of the sprite banks, against their decoded bytes",
        "sprite palettes", palettes.entries, palettes.both_entries, palettes.bytes, palettes.both_bytes
    );
    println!(
        "total EMI entries (data tables not counted): {both_entries} of {entries} entries, {both_bytes} of {bytes} bytes"
    );
    Ok(EXIT_OK)
}

struct Arguments {
    image: Option<PathBuf>,
    out: Option<PathBuf>,
    selection: Selection,
    list: bool,
    jobs: Option<usize>,
    text_disc: Option<PathBuf>,
    quiet: bool,
}

/// The switches of `bof3 assets extract`.
#[derive(Parser)]
struct ExtractArgs {
    #[arg(short = 'i', long = "input", alias = "image")]
    image: Option<PathBuf>,
    #[arg(short = 'o', long = "output", alias = "out")]
    out: Option<PathBuf>,
    #[arg(long)]
    only: Option<String>,
    #[arg(long)]
    mode: Option<String>,
    #[arg(long = "audio-format")]
    audio: Option<String>,
    #[arg(long)]
    list: bool,
    #[arg(long)]
    jobs: Option<String>,
    #[arg(long = "text-disc")]
    text_disc: Option<PathBuf>,
    #[arg(short = 'q', long)]
    quiet: bool,
}

fn extract_arguments(args: &[String]) -> Result<Arguments> {
    let args: ExtractArgs = parse(args)?;
    let mode = args.mode.as_deref().map(Mode::parse).transpose()?.unwrap_or_default();
    let audio = args.audio.as_deref().map(AudioFormat::parse).transpose()?.unwrap_or(AudioFormat::Wav);
    let selection = args.only.as_deref().map(Selection::parse).transpose()?.unwrap_or_else(Selection::all);
    let jobs = args
        .jobs
        .map(|jobs| {
            jobs.parse::<usize>()
                .ok()
                .filter(|jobs| *jobs > 0)
                .ok_or_else(|| Error::Usage("--jobs needs a positive number".into()))
        })
        .transpose()?;
    Ok(Arguments {
        image: args.image,
        out: args.out,
        selection: selection.with_mode(mode).with_audio(audio),
        list: args.list,
        jobs,
        text_disc: args.text_disc,
        quiet: args.quiet,
    })
}

pub(crate) fn extract_command(args: &[String]) -> Result<i32> {
    let arguments = extract_arguments(args)?;
    let base = env::current_dir()?;
    let out = arguments.out.clone().unwrap_or_else(|| base.join(DEFAULT_OUT));
    let write = !arguments.list;
    let only_text = arguments.selection.is_only(Kind::Text);

    let started = Instant::now();
    if write {
        fs::create_dir_all(&out)?;
    }
    let image = match arguments.image.clone() {
        Some(image) => Some(image),
        None if !only_text => Some(find_disc_image(&base).map_err(Error::Usage)?),
        // The text kind can read an extracted disc without the image; it only loses the LBA.
        None => find_disc_image(&base).ok(),
    };
    let mut report = if only_text {
        Default::default()
    } else {
        let options = Options {
            image: image.clone().expect("a non-text run resolves its image"),
            out: out.clone(),
            selection: arguments.selection.clone(),
            write,
            jobs: arguments
                .jobs
                .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)),
        };
        extract::run(&options)?
    };
    if arguments.selection.contains(Kind::Text) {
        add_text(&mut report, &arguments, &base, &out, image.as_deref(), write)?;
    }
    report.sort();
    if write {
        fs::write(out.join(TABLE_FILE), report.jsonl())?;
    }
    let summary = report.summary();
    if write {
        fs::write(out.join(SUMMARY_FILE), &summary)?;
    }
    if !arguments.quiet {
        print!("{summary}");
        let target = if write { out.display().to_string() } else { "(plan only)".into() };
        println!("{} rows in {:.1}s -> {target}", report.rows.len(), started.elapsed().as_secs_f64());
    }
    Ok(if report.failures.is_empty() { EXIT_OK } else { EXIT_FAILURES })
}

/// Extracts the `text` kind from the extracted disc, or notes why it did not run.
fn add_text(
    report: &mut rebof3_assets::report::Report,
    arguments: &Arguments,
    base: &Path,
    out: &Path,
    image: Option<&Path>,
    write: bool,
) -> Result<()> {
    let mode = arguments.selection.mode();
    let raw = mode.writes("raw", true);
    let modern = mode.writes("json", true);
    if !raw && !modern {
        report.notes.push("text: this mode writes no text files".into());
        return Ok(());
    }
    let only_text = arguments.selection.is_only(Kind::Text);
    let disc = arguments.text_disc.clone().unwrap_or_else(|| base.join(DEFAULT_DISC));
    let options = text::Options {
        disc,
        out: out.to_path_buf(),
        image: image.map(Path::to_path_buf),
        raw,
        modern,
        write,
        list: arguments.list,
    };
    match text::extract_with(&options) {
        Ok(text) => {
            report.rows.extend(text.rows);
            report.failures.extend(text.failures);
            report.notes.extend(text.notes);
            Ok(())
        }
        Err(error) if only_text => Err(error),
        Err(error) => {
            report.notes.push(format!("text skipped: {error}"));
            Ok(())
        }
    }
}
