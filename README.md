# bof3-tools

[![CI](https://github.com/raulcorreia7/bof3-tools/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/raulcorreia7/bof3-tools/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

`bof3` is a command-line tool for *Breath of Fire III* (US, PlayStation). It extracts and rebuilds
disc images, lists, unpacks and packs EMI archives, and converts the disc's assets to open formats.

**No game data is included.** Dump your own US disc (Redump-verified CUE sheet and BIN tracks) to use
the disc and asset commands. This is a fan project, not affiliated with or endorsed by Capcom.
*Breath of Fire* is Capcom's trademark.

## Install

From a release: download the archive for your platform from
[Releases](https://github.com/raulcorreia7/bof3-tools/releases), unpack it and run `bof3`. Each
archive has a matching `.sha256` file; check it with `sha256sum -c` (Linux) or `shasum -a 256 -c`
(macOS).

From source: you need Git and Rust 1.99 or later (`rust-toolchain.toml` selects stable Rust and
`rustup` installs it on the first build). On Linux, building the editor needs the ALSA, fontconfig,
Wayland, xkbcommon, XCB and Vulkan headers (`libasound2-dev libfontconfig1-dev libwayland-dev
libxkbcommon-dev libxkbcommon-x11-dev libxcb1-dev libxcb-render0-dev libxcb-shape0-dev
libxcb-xfixes0-dev libvulkan-dev` on Debian and Ubuntu); the `bof3` command does not. The editor
draws with [gpui](https://crates.io/crates/gpui) and needs a Vulkan driver on Linux.
Run `rustup update stable` to update an installed toolchain.

```sh
git clone https://github.com/raulcorreia7/bof3-tools.git
cd bof3-tools
make build   # writes target/release/bof3
```

## Usage

```sh
bof3 --help
bof3 disc extract -i "Breath of Fire III (USA).cue" -o build/disc
bof3 emi list build/disc/BIN/BGM/BGM000.EMI
bof3 assets extract-all -i "Breath of Fire III (USA).cue" -o build/extract
```

`emi pack` builds an EMI archive from any files, so you can try the tool without a disc:

```sh
printf hello > a.bin
printf world > b.bin
bof3 emi pack -o test.EMI -i a.bin b.bin
bof3 emi list test.EMI
```

| Command       | Does                                                           |
| ------------- | -------------------------------------------------------------- |
| `bof3 disc`   | Extract, checksum, verify and rebuild a PlayStation disc image |
| `bof3 emi`    | List, extract, pack and inspect EMI archives                   |
| `bof3 assets` | Extract the disc's assets or convert one asset                 |

## Asset editor

`bof3-edit` is a desktop editor for the disc's EMI archives. It runs on Linux (X11 and Wayland), macOS and
Windows.

```sh
cargo run --release -p rebof3-editor -- build/disc my-mod
```

The first argument is an extracted disc (the folder that holds `BIN/`), the second a project folder. The
editor only reads the disc. Edits are saved to `my-mod/edits/`, and **Project → Export** rebuilds each
edited archive into `my-mod/disc/`, then reads it back to confirm that it holds exactly the edited
payloads.

| View    | Entries                       | What you can do                                                    |
| ------- | ----------------------------- | ------------------------------------------------------------------ |
| Info    | every entry                   | type, load address, size, format and what is known about it        |
| Hex     | every entry                   | edit bytes; read and write the value at the cursor as any int width |
| Texture | VRAM uploads                  | view at 4, 8 or 16 bits with a joined or chosen palette; paint     |
| Palette | palette banks                 | edit any color's 5-bit channels and semi-transparency bit          |
| Tables  | `ETC/GAME.EMI` entry 0, `ETC/START.EMI` entry 8, area encounter blocks | spreadsheets of items, equipment, shops, abilities, level growth, party members, encounters and enemies; drops, equipment, skills and encounter slots show the name they point at |
| Text    | message banks, any strings    | rewrite a string in place (it must fit its old bytes)              |
| Sound   | VAB banks, sequence packages  | play samples; export WAVE, SoundFont and MIDI                      |
| Map     | area map bundles              | the map from above, coloured by tile bank and height; edit a cell's corner heights |
| Sprite bank | character sprite banks, raw or LZ77 | each section's node table and palettes; edit any palette color |
| Decoded | cue, scene, script, palette, LZ77 | the decoder's structured reading                              |

Undo and redo cover every view (`Ctrl+Z`, `Ctrl+Y`); `Ctrl+S` saves, `Ctrl+E` exports. The editor
uses the system's fonts; set `BOF3_EDIT_FONT` (and `BOF3_EDIT_MONO`) to the name of an installed font
family to use another one.

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

## Documentation

- [Command line](docs/cli.md): every command, switch and example.
- [Asset output](docs/assets.md): the folder layout and file kinds.
- [Crates](docs/crates.md): the libraries behind the command.
- Related projects: [bof3-player](https://github.com/raulcorreia7/bof3-player) plays the extracted
  audio, and the [bof3 workspace](https://github.com/raulcorreia7/bof3) links all projects.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md), the [code of conduct](CODE_OF_CONDUCT.md) and the
[security policy](SECURITY.md). Run `make check` before a pull request.

## License

MIT. See [LICENSE](LICENSE) and [NOTICE](NOTICE).
