# Contributing

Issues and pull requests are welcome. All contributions follow the
[code of conduct](CODE_OF_CONDUCT.md). Ask questions in
[Discussions](https://github.com/raulcorreia7/bof3-tools/discussions).

## Set up

Install the tools in the [README requirements](README.md#requirements): Rust 1.99 or later, GNU make,
`cargo-deny` and the Linux libraries. Then:

```sh
git clone https://github.com/raulcorreia7/bof3-tools.git
cd bof3-tools
git switch -c fix/short-name
```

## Checks

Run before every push:

```sh
make check
```

`make check` runs the format check, `cargo clippy`, `cargo deny` and the tests. CI runs the same checks
on Linux and Windows, checks the minimum supported Rust version (`rust-version` in `Cargo.toml`) and
builds the CLI. The release workflow also builds for macOS. A pull request that changes only docs runs
the format check only.

## Test data

The tests use synthetic fixtures built in code. Cases that need the retail disc skip when it is
absent, so you can work on every crate without one. To run them, dump your own US disc
(Redump-verified CUE sheet and BIN tracks) and keep it outside Git.

Two variables name the retail data, for every crate:

| Variable | Value |
| -------- | ----- |
| `BOF3_DISC_DIR` | the extracted disc, the folder that holds `BIN/` (`bof3 disc extract`) |
| `BOF3_DISC_IMAGE` | the disc's `.cue` sheet |

A retail test skips with a message when its variable is unset, and fails with the path when the
variable is set but wrong (no `BIN/` folder, missing cue sheet). A skip looks green, so a gate that
must prove the retail results runs `make check-retail`: it sets `BOF3_REQUIRE_RETAIL=1`, which turns
every skip into a failure, and runs the workspace tests.

Do not commit game data, disc images, extracted assets, game text or build output (`build/`, `target/`, `bin/`).

## Submit a change

1. Branch from `main` and keep each change focused.
2. Commit as `type(scope): subject`, imperative and lowercase, for example
   `fix(emi): reject a short header` or `docs(cli): add a pack example`.
3. Run `make check`, then push and open a pull request that describes the behavior and the checks.

Docs, comments and commit messages follow the workspace
[writing standard](https://github.com/raulcorreia7/bof3/blob/main/docs/standard.md#writing).

## License

Contributions are under the repository's [MIT license](LICENSE). Do not submit code you did not
write or cannot license.
