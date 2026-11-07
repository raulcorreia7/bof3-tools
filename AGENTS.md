# Agent guide

bof3-tools extracts and rebuilds *Breath of Fire III* (PlayStation) disc images and EMI archives.
It is a Cargo workspace with one CLI, `bof3`. Read [README.md](README.md) first, then
[CONTRIBUTING.md](CONTRIBUTING.md).

## Where things are

| Path                     | What it holds                                                    | Doc                                        |
| ------------------------ | ---------------------------------------------------------------- | ------------------------------------------ |
| `cli/rebof3/`            | The `bof3` binary: `disc`, `emi` and `assets` subcommands         | [docs/cli.md](docs/cli.md)                 |
| `crates/rebof3-emi/`     | The EMI archive reader and writer                                | [docs/crates.md](docs/crates.md#rebof3-emi) |
| `crates/rebof3-assets/`  | Asset conversion and the extract-all output                      | [docs/assets.md](docs/assets.md)           |
| `gui/rebof3-editor/`     | The `bof3-edit` asset editor: browse, view and edit EMI entries  | [README.md](README.md#asset-editor)        |
| `Cargo.toml`             | Workspace members and the pinned `bof3player` crates         | [docs/crates.md](docs/crates.md)      |
| `Makefile`               | `build`, `test`, `format`, `lint`, `check`, `clean`              | `make help`                                |

## Checks

Run from this folder before every report. CI runs the same checks:

```sh
make check
```

`make check` runs the format check, `cargo clippy`, `cargo deny` and the tests.

## Rules

- Commit no game data, disc images, extracted assets or game text. Tests use synthetic fixtures;
  a test that needs the retail disc skips without it.
- A task's `writes` list is the grant. Anything else is a `needs:` line in the report.
- Commit as `type(scope): subject`, imperative and lowercase, for example
  `fix(emi): reject a short header`. Add no AI attribution. Never amend, force-push or rewrite
  history.

## Index

- Skills: none; this guide is the entry point.
- Tools: `bof3 --help` and [docs/cli.md](docs/cli.md); `make help`.
- Docs: [docs/](docs/) (table above).
- CI: [.github/workflows/ci.yml](.github/workflows/ci.yml) runs `make check`;
  [release.yml](.github/workflows/release.yml) builds the release archives.
- Wiki (game and format facts): [site](https://raulcorreia7.github.io/bof3-wiki/). For a model: [llms.txt](https://raulcorreia7.github.io/bof3-wiki/llms.txt) (index), [llms-small.txt](https://raulcorreia7.github.io/bof3-wiki/llms-small.txt) (compact, for a small context), [llms-full.txt](https://raulcorreia7.github.io/bof3-wiki/llms-full.txt) (everything).
- Project standard: [docs/standard.md](https://github.com/raulcorreia7/bof3/blob/main/docs/standard.md).

## Writing

Docs, comments, commit messages and pull requests follow the [project standard](https://github.com/raulcorreia7/bof3/blob/main/docs/standard.md#writing): present tense, what and why instead of how, ASD-STE100 and Orwell's six rules.
Comments say what, never how: one line, only where a name cannot say it ([standard](https://github.com/raulcorreia7/bof3/blob/main/docs/standard.md#comments)).
