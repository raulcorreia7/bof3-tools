# rebof3

The *Breath of Fire III* toolchain command line: one binary with `player`, `disc`, `emi` and
`assets`. The subcommands parse arguments and print; the libraries hold the logic.

```sh
cargo build --release -p rebof3
cargo test -p rebof3
bin/rebof3 --help
```

Commands, flags and `extract-all` steps: [the rebof3 command line](../../wiki/src/tools/cli.md).

## Notice

A fan project, not affiliated with or endorsed by Capcom. Breath of Fire is a Capcom trademark. No
game data is included: bring your own disc.
