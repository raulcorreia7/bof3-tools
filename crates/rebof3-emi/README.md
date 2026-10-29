# rebof3-emi

The EMI container and the game formats stored in it: packing, `emi.json` manifests, LZ77, area
bundles, type-8 cue tables, classification and coverage. The PlayStation formats inside the entries
(VAB, SEP, VRAM, CLUT) are in `psx-formats`; the `rebof3` command line exposes the container verbs.

```sh
cargo test -p rebof3-emi
```

Container, manifest contract, module map and tests:
[rebof3-emi](https://github.com/raulcorreia7/bof3-monorepo/blob/main/wiki/src/tools/crates.md#rebof3-emi).

## Notice

A fan project, not affiliated with or endorsed by Capcom. Breath of Fire is a Capcom trademark. No
game data is included: bring your own disc.
