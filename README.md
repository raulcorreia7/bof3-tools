# bof3 tools

Rust tools for inspecting and extracting Breath of Fire III (US, PlayStation) disc images,
EMI archives and assets. This repository contains no game data. Dump your own US disc
(Redump-verified CUE sheet and BIN tracks) to use the disc and asset commands.

## Build

You need Git and Rust. `rust-toolchain.toml` pins the toolchain, and `rustup` installs it on the first
build. On Linux, audio extraction needs the ALSA headers.

```sh
git clone https://github.com/raulcorreia7/bof3-tools.git
cd bof3-tools
make build   # writes target/release/bof3
make check   # format, clippy, cargo deny and tests; needs cargo-deny
```

## First command

```sh
target/release/bof3 --help
```

The command list and the guides are in [command line](docs/cli.md), [crates](docs/crates.md) and
[asset output](docs/assets.md).

## Example without a disc

`emi pack` builds an EMI archive from any files, and `emi list` prints its entry table:

```sh
printf hello > a.bin
printf world > b.bin
target/release/bof3 emi pack -o test.EMI -i a.bin b.bin
target/release/bof3 emi list test.EMI
```

## With a disc

```sh
target/release/bof3 disc extract -i "Breath of Fire III (USA).cue" -o build/disc
```

The [bof3-player](https://github.com/raulcorreia7/bof3-player) project provides the audio player. The workspace depends on its public repository.
See [CONTRIBUTING.md](CONTRIBUTING.md) to change the tools.

The tools are MIT licensed. See [NOTICE](NOTICE).
