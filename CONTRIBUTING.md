# Contributing

All contributions follow the [code of conduct](CODE_OF_CONDUCT.md). Contributions are under the
repository's [MIT license](LICENSE).

## Set up

You need Git and Rust. `rust-toolchain.toml` pins the toolchain, and `rustup` installs it on the first
build. Audio extraction needs the ALSA headers on Linux. The README shows how to build and run
`bof3`.

```sh
git clone https://github.com/raulcorreia7/bof3-tools.git
cd bof3-tools
make check
```

`make check` runs the format check, `cargo clippy`, `cargo deny` and the tests. Install
`cargo-deny` once with `cargo install cargo-deny`. CI runs the same checks.

## Disc and test data

The tests use synthetic fixtures built in code. Cases that need the retail disc skip when it is
absent, so you can work on every crate without one. To run them, dump your own US disc
(Redump-verified CUE sheet and BIN tracks) and keep it outside Git.

Do not commit game data, disc images, extracted assets or game text.

## Submit a change

Create a branch and keep each change focused. Use commit subjects such as
`fix(emi): reject a short header` or `docs(cli): add a pack example`. Describe the behavior and the
checks in the pull request.
