# Changelog

## 1.0.0 (unreleased)

One `rebof3` binary with four subcommands. `player` is a thin caller into the
`bof3player` binary/library; the standalone `bof3player` is the public form of the
same command.

### Player

- `render --format flac|ogg` writes lossless FLAC or Ogg Vorbis; Ogg `--quality` is 0-10 and
  defaults to 6 (about 192 kbps). The export menu offers WAV, FLAC and Ogg for every playable kind.
- `--loops N` counts plays, including the first, and defaults to 1.
- FLAC and Ogg music default to the intro plus one loop and carry `LOOPSTART`/`LOOPLENGTH` (sample
  frames), `ARTIST`/`ALBUM` (`Breath of Fire III`) and `TRACKNUMBER` tags. The export menu chooses
  low (96 kbps), normal (192 kbps) or high (320 kbps) Ogg quality.
- `rebof3 player play FILE.ogg|FILE.flac` decodes and plays a file the user points it at.

### Assets

- `assets extract --audio-format wav|flac|ogg` (default `wav`) renders every playable music,
  sample, XA and CD-DA item in that container; `assets.jsonl` names the chosen extension. Music is
  rendered as audio, with its `.mid` beside it; its `.seq` stays among the stored bytes.

### Migration

| Removed                  | Replacement          |
| ------------------------ | -------------------- |
| `player ...` (binary)    | `rebof3 player ...`  |
| `disc ...` (binary)      | `rebof3 disc ...`    |
| `emi ...` (binary)       | `rebof3 emi ...`     |
| `assets ...` (binary)    | `rebof3 assets ...`  |
| `bin/player`, `bin/disc`, `bin/emi`, `bin/assets` | `bin/rebof3` |
