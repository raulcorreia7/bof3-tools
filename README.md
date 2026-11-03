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

From source: you need Git and Rust 1.99 or later (`rust-toolchain.toml` pins the toolchain and
`rustup` installs it on the first build). On Linux, audio extraction needs the ALSA headers
(`libasound2-dev`).

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
