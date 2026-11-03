# bof3 command line

The *Breath of Fire III* toolchain command line: one binary with `disc`, `emi` and
`assets`. The subcommands parse arguments and print; the libraries hold the logic.

```sh
cargo build --release -p rebof3   # the Cargo package; the binary is `bof3`
cargo test -p rebof3
target/release/bof3 --help
```

Commands, flags and `extract-all` steps: [the bof3 command line](../../docs/cli.md).

## Notice

A fan project, not affiliated with or endorsed by Capcom. Breath of Fire is a Capcom trademark. No
game data is included: bring your own disc.
