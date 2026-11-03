# Changelog

## 1.0.0 (unreleased)

One `bof3` binary with `disc`, `emi` and `assets` subcommands. The audio player is `bof3player`.

### Assets

- `assets extract --audio-format wav|flac|ogg` (default `wav`) renders every playable music,
  sample, XA and CD-DA item in that container; `assets.jsonl` names the chosen extension. Music is
  rendered as audio, with its `.mid` beside it; its `.seq` stays among the stored bytes.

### Migration

| Removed                                           | Replacement       |
| ------------------------------------------------- | ----------------- |
| `rebof3 player ...`                               | `bof3player ...`  |
| `disc ...` (binary)                               | `bof3 disc ...`   |
| `emi ...` (binary)                                | `bof3 emi ...`    |
| `assets ...` (binary)                             | `bof3 assets ...` |
| `bin/player`, `bin/disc`, `bin/emi`, `bin/assets` | `bin/bof3`        |
