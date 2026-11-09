# The bof3 command line

`bof3` provides the `disc`, `emi` and `assets` commands. The subcommands parse arguments and
print; the libraries hold the logic ([crates](crates.md)). `--help` lists each command's purpose,
every switch with its default, and runnable examples. A command with verbs (`disc extract`,
`emi pack`, `assets import texture`) has its own focused `--help`. Errors name what failed and the
next safe action.

```sh
make build
target/release/bof3 --help
target/release/bof3 disc extract -i "Breath of Fire III (USA).cue" -o build/disc
target/release/bof3 emi list "BIN/BGM/BGM000.EMI"
target/release/bof3 assets extract-all -o build/extract
```

`make build` writes `target/release/bof3`. The build needs the Rust toolchain in
`rust-toolchain.toml`. Audio extraction uses the [bof3-player](https://github.com/raulcorreia7/bof3-player) libraries;
on Linux they need the ALSA headers. The examples run from the repository root and write below `build/`.

| Command       | Does                                                           | Reference         |
| ------------- | -------------------------------------------------------------- | ----------------- |
| `bof3 disc`   | Extract, checksum, verify and rebuild a PlayStation disc image | [Disc](#disc)     |
| `bof3 emi`    | List, extract, pack and inspect EMI archives                   | [EMI](#emi)       |
| `bof3 assets` | Extract the disc's assets or convert one asset                 | [Assets](#assets) |

Switch names match across commands: `-i/--input`, `-o/--output`, `--list`, `--json`, `-q/--quiet`.
`-i` for a disc image defaults to the image in `inputs/external`, then `./disk`.

## disc

`disc` extracts, checksums, verifies and describes disc images.

```sh
target/release/bof3 disc extract -i inputs/external/disc.cue -o build/disc
target/release/bof3 disc checksum -i build/disc -o build/checksums.json
target/release/bof3 disc verify -i build/disc -o build/checksums.json
```

| Verb       | Switches                                                                                                            |
| ---------- | ------------------------------------------------------------------------------------------------------------------- |
| `extract`  | `-i IMAGE`, `-o DIR` (default `build/extracted`), `-p FILE.xml` (default `DIR/IMAGE.xml`), `-q`                     |
| `rebuild`  | `-i DIR` and `-o IMAGE.iso`, both required; writes a synthetic ISO of the folder's top-level files, not a game disc |
| `lba-json` | `-i IMAGE`, `-o FILE.json`, `-x DIR`, `-p FILE.xml` (all required), `-r DIR` (default `processed/emi_raw`)          |
| `checksum` | `-i DIR`, `-o FILE.json`, both required                                                                             |
| `verify`   | `-i DIR`, `-o FILE.json`, both required                                                                             |

Every verb takes `-q/--quiet`. `verify` exits 0 when every file matches, 1 on a mismatch and 2 on an
error.

## emi

`emi` lists and extracts EMI archives, packs an archive from files or a folder, and inspects image,
palette, bank and sequence entries. The container layout is in the
[EMI format](https://raulcorreia7.github.io/bof3-wiki/formats/emi.html).

```sh
target/release/bof3 emi list "BIN/BGM/BGM000.EMI"
target/release/bof3 emi extract "BIN/BGM/BGM000.EMI" -o build/bgm000
target/release/bof3 emi pack -o build/test.EMI build/src
target/release/bof3 emi vram -i "BIN/BGM/BGM000.EMI" -o build/page.png
```

`bof3 emi ARCHIVE.EMI` extracts the archive. Every verb takes the archive as `-i/--input` or as the
first positional.

| Verb         | Switches                                                                                                          |
| ------------ | ----------------------------------------------------------------------------------------------------------------- |
| `list`       | `--json` prints a JSON array, one object per entry                                                                |
| `extract`    | `-o PATH`, `-n INDEX` (one entry), `-e` (typed extensions), `-J [FILE]` (write `emi.json`), `-d` (dry run), `-q`  |
| `pack`       | `-o ARCHIVE` (required), `-i FILE...`, `-t TYPE` (default 255), `-I/-X PATTERN`, `-J [FILE]`, `-k/-K`, `-d`, `-q` |
| `vram`       | `-o PAGE.png`; decodes the type-3 uploads into one page set                                                       |
| `clut`       | `INDEX`, `-j FILE.json`, `-p FILE.png`                                                                            |
| `vab`, `sep` | `INDEX`, `--dump` (every field; default is a summary)                                                             |
| `coverage`   | `-i DIR` (default `raw/disc/BIN`), `-o FILE` (JSONL table), `--json`                                              |

`extract` validates the header, the table bounds, the payload bounds and the entry index before it
writes. `-I` and `-X` filter the files of a folder pack (`-X` defaults to `emi.json`). A bare `-J` must be
the last argument, because the next argument is read as its path; without a value it means the
folder's `emi.json`.

### Manifest packing contract

`pack FOLDER -J [FILE]` loads and validates every named entry once, before it reads a payload or
creates the output. The manifest supplies each entry's type, RAM pointer and the archive version.
Without `-J`, `pack` guesses the type from the extension and loses the load addresses.

- A name that is absolute, has a parent-directory component, is missing, is not a regular file, or
  resolves outside the canonical input folder (a symlink escape) fails the run with exit 2 and a
  message naming the entry. Two names that resolve to the same file inside the folder are both
  packed.
- `-d` runs the same validation, writes nothing and deletes nothing.
- An entry without a non-empty `name` is a metadata row and is not packed. Manifest order and
  duplicate names are kept, so a repeated name becomes a second archive entry with its own type and
  RAM pointer.
- Packing keeps its inputs by default. `-K/--no-keep` deletes the inputs that were validated and
  packed, once each. It resolves every path again first and refuses one that no longer resolves to
  itself inside the folder. A cleanup failure names the path, exits 2 and leaves the finished
  archive in place.
- `-i FILE...` keeps its caller-selected scope: those paths are validated as regular files and are
  the only ones cleanup deletes.

## assets

`assets` extracts assets from the disc and converts one asset between its stored bytes and an open
format. The kinds and the output layout are in [assets](assets.md).

```sh
target/release/bof3 assets extract --only texture,palette -o build/art
target/release/bof3 assets extract --only audio-sequence --audio-format ogg --mode modern -o build/music
target/release/bof3 assets extract-all -o build/extract
target/release/bof3 assets coverage -i "Breath of Fire III (USA).cue"
```

`assets extract` writes to `build/game-assets` by default.

| Switch               | Meaning                                                                              |
| -------------------- | ------------------------------------------------------------------------------------ |
| `-i/--input IMAGE`   | CUE sheet or image; `--image` is an alias                                            |
| `--only KINDS`       | Comma-separated kinds (default every kind)                                           |
| `--mode raw\|modern` | `raw` writes the stored bytes, `modern` the conversions; the default writes both     |
| `--audio-format FMT` | `wav`, `flac` or `ogg` for rendered audio (default `wav`)                            |
| `--list`             | Classify everything and write nothing                                                |
| `--jobs N`           | Worker threads (default every core)                                                  |
| `--text-disc DIR`    | Extracted disc the `text` kind reads (default `build/disc` in the working directory) |

`assets kinds` lists every kind and what it writes. `assets coverage [-i IMAGE]` reports which kinds
rebuild their own bytes ([round-trip coverage](assets.md#round-trip-coverage)).

The `text` kind reads the extracted disc: it walks every `.EMI` archive below `--text-disc`, and each
payload that holds text writes `text/<disc path without .EMI>/<slot>.bin`, `<slot>.json` and
`<slot>.txt`, plus `text/index.jsonl`. `-i IMAGE` supplies the archive LBA for the JSON's disc offset.
`--list` prints the plan and every payload the scan drops. The file contents are in
[assets](assets.md#text).

### Converting one asset

| Operation | Verbs                                                                                                                                                                                     |
| --------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Export    | `terrain` (glTF or OBJ), `soundfont`, `tables`, `encounters`, and `export` for `texture`, `palette`, `terrain`, `vag`, `sequence`, `message`, `message-po`, `script`, `strings`, `characters` or `character-bank` |
| Import    | `import` for `texture`, `palette`, `terrain`, `vag`, `table`, `encounters`, `layout`, `emi`, `sequence`, `disc`, `soundfont`, `message`, `message-po`, `script`, `strings`, `characters` or `character-bank`      |

Each verb has focused help with its own switches and defaults, for example
`bof3 assets export texture --help` and `bof3 assets import disc --help`. Common export switches are
`-o/--output`, `--load-arg HEX`, `--depth 4|8`, `--palette BANK`, `--palette-offset WORDS` and
`--index N`. Common import switches are `-o/--output`, `--payload PATH`, `--palette OUT`, `--base BLOCK`,
`--entry INDEX=FILE` and `--file PATH=FILE`.

```sh
target/release/bof3 assets export texture payload.img --load-arg 0x0a081000 -o 7.png
target/release/bof3 assets import texture 7.png -o payload.img
```

`terrain` writes `.glb` for a `.glb` output path and OBJ plus MTL otherwise. `soundfont` takes the header
and body that share a load argument. `import disc` replaces files of the disc by path
(`--file PATH=FILE`); a file cannot grow past its last sector. `import encounters` requires `--base`.
Import behavior and round-trip results are in [assets](assets.md#import).

### extract-all

`assets extract-all [-i IMAGE] [-o DIR] [--jobs N] [-q] [STEP...]` runs five steps and writes one
folder per step below `DIR` (default `build/extract`).

| Step            | Writes                                                          |
| --------------- | --------------------------------------------------------------- |
| `disc`          | the disc's files as stored                                      |
| `emi`           | each `.EMI` archive, one folder per archive, typed extensions   |
| `assets`        | classified payloads, the text, `assets.jsonl` and `summary.txt` |
| `assets-export` | terrain glTF, message JSON, SoundFont and MIDI                  |
| `audio`         | sample WAVs rendered by the audio library                       |

`DIR/index.json` and `DIR/index.md` list every file with its step, kind, size and SHA-256. Named steps
rerun only those steps; the others keep their files and index rows. `assets` needs the folder the
`disc` step writes. Exit status 0 is success, 1 means a step had failures, and 2 an error.
