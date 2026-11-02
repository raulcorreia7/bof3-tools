# Library crates

The workspace holds the game crates and the command line. The audio libraries come from the public
[bof3player](https://github.com/raulcorreia7/bof3player) repository.

| Crate           | What it is                                                                |
| --------------- | ------------------------------------------------------------------------- |
| `rebof3-emi`    | EMI container, game records and area formats, LZ77, cue tables, coverage. |
| `bof3-names`    | The game catalog names in the bof3player repository.                      |
| `rebof3-assets` | The all-assets extractor: classify every file and EMI slot, write a tree. |
| `rebof3`        | The command line; argument parsing and printing only.                     |

Every first-party crate is MIT; `cargo deny check` keeps dependencies permissive. The container layout
is in the [EMI format](https://raulcorreia7.github.io/bof3-wiki/formats/emi.html). The PlayStation formats inside the entries (VAB, SEP,
VRAM, CLUT) are in
[psx-formats](https://github.com/raulcorreia7/bof3player/blob/main/docs/crates.md#psx-formats).

## rebof3-emi

The EMI container and the game formats stored in it: packing, `emi.json` manifests, LZ77, area
bundles, type-8 cue tables, classification and coverage. Without the `std` feature the crate is
`no_std + alloc` and keeps the container table and the cue table, which the audio library reads. The
extractor validates the header, the table bounds, the payload bounds and the entry index before it
writes. The packer reproduces the retail fill bytes
([padding](https://raulcorreia7.github.io/bof3-wiki/formats/emi.html#padding)). The packing contract is in
[the command line](cli.md#manifest-packing-contract).

## bof3-names

Embeds the names file `bof3player` reads, as a
[`Naming`](https://github.com/raulcorreia7/bof3player/blob/main/docs/rust-api.md#names-and-titles)
implementation for the tools. The file maps archive paths and ids to display names, soundtrack
titles, magical abilities, enemy banks and duplicate groups.

## rebof3-assets

The all-assets extractor: reads the disc through `psx-cd`, classifies every file and every EMI slot
with `rebof3-emi`, and writes one tree of assets plus `assets.jsonl` and `summary.txt`. The output
layout and the round trips are in [assets](assets.md); the kinds and joins are in the
[asset extraction format page](https://raulcorreia7.github.io/bof3-wiki/formats/assets.html). The tests run synthetic disc cases and, when
the retail image is present, a pinned inventory of every kind and a digest of every stored byte; the
retail cases skip without the disc.
