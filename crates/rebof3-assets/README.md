# rebof3-assets

The all-assets extractor: it reads the disc through `psx-cd`, classifies every file and every EMI
slot with `rebof3-emi`, and writes one tree of assets with `assets.jsonl` and `summary.txt`. The
decoded views, imports, coverage and tests build on the same stored bytes.

```sh
cargo test -p rebof3-assets
```

Modules, output contract and tests:
[rebof3-assets](../../docs/crates.md#rebof3-assets); the output layout and round trips:
[asset output](../../docs/assets.md).

## Notice

A fan project, not affiliated with or endorsed by Capcom. Breath of Fire is a Capcom trademark. No
game data is included: bring your own disc.
