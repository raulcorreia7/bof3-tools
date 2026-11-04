# Contributing

Issues and pull requests are welcome. All contributions follow the
[code of conduct](CODE_OF_CONDUCT.md). Ask questions in
[Discussions](https://github.com/raulcorreia7/bof3-tools/discussions).

## Set up

You need Git and Rust 1.99 or later. `rust-toolchain.toml` pins the toolchain, and `rustup`
installs it on the first build. Audio extraction needs the ALSA headers on Linux. The
[README](README.md) shows how to build and run `bof3`.

```sh
git clone https://github.com/raulcorreia7/bof3-tools.git
cd bof3-tools
make check
```

## Checks

`make check` runs the format check, `cargo clippy`, `cargo deny` and the tests. Install
`cargo-deny` once with `cargo install cargo-deny --locked`. CI runs the same checks on Linux
and Windows, the release workflow also builds for macOS, and CI builds with the minimum supported Rust version (`rust-version` in
`Cargo.toml`).

## Test data

The tests use synthetic fixtures built in code. Cases that need the retail disc skip when it is
absent, so you can work on every crate without one. To run them, dump your own US disc
(Redump-verified CUE sheet and BIN tracks) and keep it outside Git.

Do not commit game data, disc images, extracted assets or game text.

## Submit a change

Create a branch and keep each change focused. Use commit subjects such as
`fix(emi): reject a short header` or `docs(cli): add a pack example`. Describe the behavior and the
checks in the pull request.

## License

Contributions are under the repository's [MIT license](LICENSE). Do not submit code you did not
write or cannot license.
