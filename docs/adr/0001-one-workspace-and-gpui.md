# ADR 0001: one Rust workspace, and gpui for both GUIs

- Status: accepted
- Date: 2026-11-07
- Decided by: raulcorreia7 (owner)
- Applies to: `bof3-tools`, `bof3-player`, and the `decomp` submodule pin

## Context

- `bof3-tools` depends on `bof3-player` by git tag. The pin is `v0.1.3`. Every player change
  needs a tag, then a bump in `Cargo.toml` and `deny.toml` of `bof3-tools`, then a submodule pin
  in `decomp`.
- The two repositories have two lockfiles, two MSRVs (1.99 in `bof3-tools`, 1.88 in
  `bof3-player`), two `[workspace.lints]` tables and two `deny.toml` files.
- `emi` (player, the container table and cue records) is wrapped by `rebof3-emi` (tools) with a
  newtype `Archive` and a second error type.
- The player GUI uses `gpui =0.2.2`: 689 unique crates, two vendored patched crates and ten
  ignored RUSTSEC entries. The asset editor `bof3-edit` uses `eframe 0.37` (egui). Two toolkits
  mean two GUI dependency trees and two sets of duplicate versions.

## Decision 1: repository layout — option A, one repository

One repository, `bof3-rs`, with one Cargo workspace:

```text
bof3-rs/
  Cargo.toml  Cargo.lock  rust-toolchain.toml  deny.toml  rustfmt.toml
  crates/   psx-formats  psx-cd  psx-audio  bof3-emi  bof3-assets  bof3-catalog
  apps/     bof3  bof3-player  bof3-player-gui  bof3-edit
  vendor/   only while a patched dependency is needed
  docs/
```

- History moves with `git subtree`, so `git log` and `git blame` keep working for both trees.
- `emi` and `rebof3-emi` merge into `bof3-emi`, with one `Archive` type and one error type.
- One MSRV: the highest that any member needs (1.99 today).
- One `deny.toml`, one CI, one `[workspace.dependencies]`, one `[workspace.lints]`.
- `decomp` pins one submodule, `bof3-rs`, instead of `bof3-tools`.

Rejected: B (publish `psx-*` and `emi` to crates.io) adds release work for crates that nobody else
uses and keeps two lockfiles. C (a bot that bumps the tag) keeps the split.

## Decision 2: GUI toolkit — option B, gpui for both

- The asset editor is ported from eframe/egui to gpui and reuses the player's gpui patterns
  (`core/src/gui/*`: theme, widgets, list, icons).
- The player keeps gpui and its vendored patches, with the exact pin `gpui = "=0.2.2"`. The exact
  pin stays because gpui 0.x changes its API between releases and both patches below target the
  versions that 0.2.2 resolves.
- Until the repository merge, `bof3-tools` gets the same patches from the `bof3-player` git tag
  (`[patch.crates-io]` entries pointing at the tagged repository), so the vendored sources are not
  copied twice. After the merge, `vendor/` lives once at the root of `bof3-rs`.

Rejected: A (egui for both) would remove the vendored crates and most advisory ignores, and was
the draft's recommendation; the owner chose gpui. C (keep both) keeps two GUI trees.

### Vendored patches and their exit conditions

| Patch | What it changes | Why | Exit condition |
| ----- | --------------- | --- | -------------- |
| `vendor/taffy` (taffy 0.9.0) | The `grid` dependency moves from 0.18.0 to 1.0.1; the rest is the crates.io source | gpui 0.2.2 pins taffy 0.9.0, and grid 0.18 has a vulnerability report (Dependabot alert 1) | Remove when the gpui release in use depends on a taffy release that uses a fixed `grid` (1.0 or later) |
| `vendor/stacksafe-macro` (0.1.4) | `proc-macro-error2` is replaced by `proc-macro-error3` | gpui requires `stacksafe` 0.1, and its macro crate used the unmaintained `proc-macro-error2` (RUSTSEC-2026-0173) | Remove when `stacksafe-macro` on crates.io drops `proc-macro-error2`, or when gpui drops `stacksafe` |

Each removal is its own pull request: delete the `[patch.crates-io]` entry and the `vendor/`
folder, then `cargo update -p <crate>` and `cargo deny check`.

### Ignored advisories that gpui brings

`RUSTSEC-2025-0052` (async-std), `RUSTSEC-2024-0384` (instant), `RUSTSEC-2026-0441` (pin-utils),
`RUSTSEC-2024-0436` (paste), `RUSTSEC-2025-0134` (rustls-pemfile), `RUSTSEC-2026-0206`
(rustybuzz), `RUSTSEC-2026-0192` (ttf-parser), `RUSTSEC-2026-0371` (zbus_names),
`RUSTSEC-2026-0370` (zvariant), `RUSTSEC-2026-0372` (zvariant_derive). Each is an unmaintained
notice, not a vulnerability. Remove an entry when the gpui tree no longer contains the crate; a
gpui upgrade re-checks the whole list.

## Consequences

- Work order: port the editor to gpui (wave 3.5), then merge the repositories (wave 3.6), so the
  merged workspace has one GUI dependency set.
- The editor port is split into reviewable pull requests: a gpui shell (window, theme, fonts,
  icon, archive browser) first, then the views (info, texture and palette paint, text edit, audio,
  project save and rebuild). Every feature and the look stay: dark theme, one accent colour,
  format tags and the window icon.
- `bof3-tools` CI needs the gpui system libraries on Linux, as the player CI does, and the eframe
  advisory ignores (AccessKit's zbus crates) become the gpui ones listed above.
- Releases move from `v0.1.x` tags on `bof3-player` and `bof3-tools` to one tag on `bof3-rs`.
  `bof3-player` and `bof3-tools` are archived after `bof3-rs` is live and `decomp` pins it.
- Creating `bof3-rs` and archiving the old repositories need a separate owner go-ahead.
- Risk: asset bytes are covered by the `rebof3-assets` tests; the GUIs and the player render path
  have no retail-data test in CI.
