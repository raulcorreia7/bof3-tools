# rebof3 tools

Rust tools for inspecting and extracting Breath of Fire III (US, PlayStation) disc images,
EMI archives and assets. Supply your own disc; this repository contains no game data.

Run `make build` to build `target/release/rebof3`, and `make check` for the workspace checks.
Run `rebof3 --help` for commands. The `bof3player` project provides the audio player.
The workspace depends on the public bof3player repository and requires Rust and Cargo.

The tools are MIT licensed. See [NOTICE](NOTICE).
