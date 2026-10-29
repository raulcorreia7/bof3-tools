# Changelog

## 0.2.0 (unreleased)

The project prefix reaches the package: `bof3-assets` is now `rebof3-assets`. No type, function
or module name inside the crate moves.

### Rust

- The text run index is now `text/index.jsonl`, with one JSON object per string.

- `extract` takes `Selection::with_audio(AudioFormat::{Wav, Flac, Ogg})` and renders every playable
  music, sample, XA and CD-DA item in that container. Music keeps its MIDI file next to the chosen
  audio file; the `.seq` package stays among the stored bytes.
- Texture extraction now writes the palette-applied view for every texture with a proven draw
  rule: `decode::joined_rows_in` takes the resident palette banks, joins the world-map terrain
  rows, and the extractor reads each selected archive's palette band to build that set.
- The `text` kind writes the stored payload (`<slot>.bin`), one JSON row per string with its source
  chain, and a plain-text view, plus `text/index.tsv` for the run. The scan keeps message banks,
  the known tables, resource tables and code menus, and drops payloads that only decode as stray
  tags; `--list` prints each dropped payload.

### Migration

| Removed                       | Replacement         |
| ----------------------------- | ------------------- |
| `bof3-assets` (Cargo package) | `rebof3-assets`     |
| `bof3_assets` (Rust path)     | `rebof3_assets`     |
| `assets ...` (command)        | `rebof3 assets ...` |

## 0.1.0

Initial all-assets extractor: walks the disc and every EMI slot, writes the asset tree with
`assets.jsonl` and `summary.txt`, and converts the stored bytes to and from the open formats.
