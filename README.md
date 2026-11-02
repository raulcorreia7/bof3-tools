# rebof3 tools

Rust tools for inspecting and extracting Breath of Fire III (US, PlayStation) disc images,
EMI archives and assets. This repository contains no game data. Dump your own US disc
(Redump-verified CUE sheet and BIN tracks) to use the disc and asset commands.

## Build

You need Git and Rust. `rust-toolchain.toml` pins the toolchain, and `rustup` installs it on the first
build. On Linux, audio extraction needs the ALSA headers.

```sh
git clone https://github.com/raulcorreia7/rebof3-tools.git
cd rebof3-tools
make build   # writes target/release/rebof3
make check   # format, clippy, cargo deny and tests; needs cargo-deny
```

## First command

```sh
target/release/rebof3 --help
```

The command list and the guides are in [command line](docs/cli.md), [crates](docs/crates.md) and
[asset output](docs/assets.md).

## Example without a disc

`emi pack` builds an EMI archive from any files, and `emi list` prints its entry table:

```sh
printf hello > a.bin
printf world > b.bin
target/release/rebof3 emi pack -o test.EMI -i a.bin b.bin
target/release/rebof3 emi list test.EMI
```

## With a disc

```sh
target/release/rebof3 disc extract -i "Breath of Fire III (USA).cue" -o build/disc
```

The `bof3player` project provides the audio player. The workspace depends on its public repository.
See [CONTRIBUTING.md](CONTRIBUTING.md) to change the tools.

The tools are MIT licensed. See [NOTICE](NOTICE).
