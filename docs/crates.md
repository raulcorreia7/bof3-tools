# Library crates

The libraries behind the `bof3` command: five crates in this workspace and the public
[bof3-player](https://github.com/raulcorreia7/bof3-player) repository, which supplies the audio,
disc and bundled-names libraries.

| Crate           | What it is                                                                |
| --------------- | ------------------------------------------------------------------------- |
| `rebof3-emi`    | EMI container, game records and area formats, LZ77, cue tables.           |
| `rebof3-formats`| One module per entry format (codec, open files, description), the registry and the coverage table. |
| `rebof3-assets` | The all-assets extractor: classify every file and EMI slot, write a tree. |
| `rebof3-render` | Draws field scenes with the GPU's texturing rules, offscreen through OpenGL ES (the `gles` feature) or in software ([ADR 0003](adr/0003-area-3d-rendering.md)). |
| `rebof3`        | The `bof3` command line; argument parsing and printing only.              |

Every first-party crate is MIT; `cargo deny check` keeps dependencies permissive. The container
layout is in the [EMI format](https://raulcorreia7.github.io/bof3-wiki/formats/emi.html). The
PlayStation formats inside the entries (VAB, SEP, VRAM, CLUT) are in
[psx-formats](https://github.com/raulcorreia7/bof3-player/blob/main/docs/crates.md#psx-formats).

## rebof3-emi

The EMI container and the primitives the format modules share: packing, `emi.json` manifests, LZ77,
the record readers, the text codec, LZSS, the VRAM model with its load replay and the container
round trip (`verify`). Without the `std` feature the crate is
`no_std + alloc` and keeps the container table and the cue table, which the audio library reads. The
extractor validates the header, the table bounds, the payload bounds and the entry index before it
writes. The packer reproduces the retail fill bytes
([padding](https://raulcorreia7.github.io/bof3-wiki/formats/emi.html#padding)). The packing
contract is in [the command line](cli.md#manifest-packing-contract).

## rebof3-formats

One module per entry format, with the registry that selects and describes them. Each format
module owns its `Description` (name, selector, extension, evidence, round-trip check); the
registry lists them, so the classifier, the coverage table, the extractor and the editor read one
table instead of matching on formats. The rules a module follows are in
[ADR 0002](adr/0002-self-contained-formats.md). The coverage table runs every format's round trip
over the retail archives and pins the totals in `tests/retail_coverage.rs`.

Beside the formats it holds four shared modules that are not formats: `joins` (a texture's palette
rows, read over a whole archive), `text` (the string scan, the strings of a payload and the sources
of a string), `gltf`, and `field` (the area setup stream that places the actors, and the
executable's encounter zones and facing steps). The SoundFont conversion is in `vab::soundfont`, which is why the crate
depends on `bof3player` for the SPU envelope steps.

## rebof3-assets

The all-assets extractor: reads the disc through `psx-cd`, classifies every file and every EMI slot
with `rebof3-emi`, and writes one tree of assets plus `assets.jsonl` and `summary.txt`. The output
layout and the round trips are in [assets](assets.md); the kinds and joins are in the
[asset extraction format page](https://raulcorreia7.github.io/bof3-wiki/formats/assets.html). The
tests run synthetic disc cases and, when the retail image is present, a pinned inventory of every
kind and a digest of every stored byte; the retail cases skip without the disc.

The `codec` module is the table of conversions between stored bytes and an open form: one row
per kind, with an export and an import op that name their files, options, error texts, default
extension and function. `bof3 assets export` and `import` parse their arguments with `clap` and
run the row of the kind named; their `--help` texts are hand-written and pinned by
`cli/rebof3/tests/help.snap`.
