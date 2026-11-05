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
| `Cargo.toml`             | Workspace members and the pinned `bof3player` and `bof3-names` crates | [docs/crates.md](docs/crates.md)      |
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

## Writing

Docs, comments, commit messages and pull requests follow one standard.

- **Current.** Present tense, no history.
- **What, not how.** Say what a thing does and why. The code shows how.
- **ASD-STE100** is the basis for technical text: short sentences, one instruction per sentence,
  active voice, one term for one concept, numbered steps for a procedure. Exact technical terms
  stay.
- **Orwell's six rules** ("Politics and the English Language", 1946):
  1. Never use a metaphor, simile or other figure of speech that you are used to seeing in print.
  2. Never use a long word where a short one will do.
  3. If it is possible to cut a word out, always cut it out.
  4. Never use the passive where you can use the active.
  5. Never use a foreign phrase, a scientific word or a jargon word if you can think of an everyday
     English equivalent.
  6. Break any of these rules sooner than say anything outright barbarous.
- Comments say what, never how: one line, only where a name cannot say it ([standard](https://github.com/raulcorreia7/bof3/blob/main/docs/standard.md#comments)).
