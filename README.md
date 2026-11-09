# bof3-tools

[![CI](https://github.com/raulcorreia7/bof3-tools/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/raulcorreia7/bof3-tools/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

`bof3` is a command-line tool for *Breath of Fire III* (US, PlayStation). It extracts and rebuilds
disc images, lists, unpacks and packs EMI archives, and converts the disc's assets to open formats.

**No game data is included.** Dump your own US disc (Redump-verified CUE sheet and BIN tracks) to use
the disc and asset commands. This is a fan project, not affiliated with or endorsed by Capcom.
*Breath of Fire* is Capcom's trademark.

## Requirements

| Need | Version | Get it |
| ---- | ------- | ------ |
| Rust (`rustup`, `cargo`) | 1.99 or later | [rustup.rs](https://rustup.rs); `rust-toolchain.toml` selects stable, and `rustup update stable` updates it |
| Git, GNU make, a C compiler, `pkg-config` | any recent | the distribution's packages |
| `cargo-deny` | any | `cargo install cargo-deny --locked` (the check gate only) |
| System libraries | Linux only | below |
| The disc | US release, serial SLUS-00422 | your own dump: a Redump-verified CUE sheet and its two BIN tracks, in one folder |

System libraries for the editor (`make build` builds it, so install them; `cargo build -p rebof3`
builds the `bof3` command with only the C compiler):

| Distribution | Packages |
| ------------ | -------- |
| Debian, Ubuntu | `build-essential pkg-config libasound2-dev libfontconfig1-dev libwayland-dev libxkbcommon-dev libxkbcommon-x11-dev libxcb1-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libvulkan-dev` |
| Arch | `base-devel alsa-lib fontconfig wayland libxkbcommon libxkbcommon-x11 libxcb vulkan-icd-loader` |

The editor draws with [gpui](https://crates.io/crates/gpui) and needs a Vulkan driver at run time.
Release archives for Linux, macOS and Windows are on
[Releases](https://github.com/raulcorreia7/bof3-tools/releases); each has a `.sha256` file to check with
`sha256sum -c` (Linux) or `shasum -a 256 -c` (macOS).

No game data is in this repository. Keep the disc dump outside Git.

## Set up

```sh
git clone https://github.com/raulcorreia7/bof3-tools.git
cd bof3-tools
```

## Build

```sh
make build
```

`make build` builds the release workspace and links `bof3` and `bof3-edit` into `bin/`. For the
command alone: `cargo build --release --locked -p rebof3`, which writes `target/release/bof3`.

## Run

Run from the repository root. `Breath of Fire III (USA).cue` stands for your CUE sheet; its BIN tracks
sit next to it.

```sh
bin/bof3 --help
bin/bof3 disc extract -i "Breath of Fire III (USA).cue" -o build/disc
bin/bof3 emi list build/disc/BIN/BGM/BGM000.EMI
bin/bof3 assets extract-all -i "Breath of Fire III (USA).cue" -o build/extract
bin/bof3-edit build/disc my-mod
```

`emi pack` builds an EMI archive from any files, so you can try the tool without a disc:

```sh
printf hello > a.bin
printf world > b.bin
bin/bof3 emi pack -o test.EMI -i a.bin b.bin
bin/bof3 emi list test.EMI
```

## Commands

| Command       | Does                                                           |
| ------------- | -------------------------------------------------------------- |
| `bof3 disc`   | Extract, checksum, verify and rebuild a PlayStation disc image |
| `bof3 emi`    | List, extract, pack and inspect EMI archives                   |
| `bof3 assets` | Extract the disc's assets or convert one asset                 |

## Asset editor

`bof3-edit` is a desktop editor for the disc's EMI archives (Linux with X11 or Wayland, macOS, Windows).
It reads an extracted disc and saves edits to a project folder. The views, keys and project layout are in
[docs/editor.md](docs/editor.md).

## Supported formats

| Input                             | Output                                                   |
| --------------------------------- | -------------------------------------------------------- |
| CUE and BIN disc image, ISO 9660  | Extracted files, checksums, a rebuilt ISO                |
| EMI archive                       | Entries by type; a packed archive from files or a folder |
| Textures, palettes                | PNG, TIM, JSON                                           |
| Sound banks, sequences, XA, CD-DA | SoundFont 2, WAV, FLAC or Ogg, MIDI                      |
| Area maps                         | OBJ, glTF                                                |
| Text, game tables, encounters     | JSON and plain text                                      |

`bof3 assets kinds` lists every asset kind and how it is handled. The formats are described in the
[wiki](https://raulcorreia7.github.io/bof3-wiki/).

## Check

```sh
make check
```

`make check` runs the format check, `cargo clippy`, `cargo deny` and the tests. Tests that need the
retail disc skip without it; `make check-retail` runs them with `BOF3_DISC_DIR` (the extracted disc) and
`BOF3_DISC_IMAGE` (the CUE sheet) set, and a skip fails ([CONTRIBUTING.md](CONTRIBUTING.md#test-data)).

## Next

- [Command line](docs/cli.md): every command, switch and example.
- [Asset output](docs/assets.md): the folder layout and file kinds.
- [Asset editor](docs/editor.md): the views, the game catalog and the format coverage.
- [Crates](docs/crates.md): the libraries behind the command.
- [CONTRIBUTING.md](CONTRIBUTING.md), the [code of conduct](CODE_OF_CONDUCT.md) and the
  [security policy](SECURITY.md).
- Related projects: [bof3-player](https://github.com/raulcorreia7/bof3-player) plays the extracted
  audio, and the [bof3 workspace](https://github.com/raulcorreia7/bof3) links all projects.

## License

MIT. See [LICENSE](LICENSE) and [NOTICE](NOTICE).
