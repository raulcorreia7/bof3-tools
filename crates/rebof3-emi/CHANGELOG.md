# Changelog

## 0.2.0 (unreleased)

The project prefix reaches the package: `bof3-emi` is now `rebof3-emi`. No type, function or
module name inside the crate moves.

### Rust

- `texture::SITES` lists the proven literal draw sites, `texture::terrain_joins` applies the
  world-map tile-descriptor rule, and `texture::ResidentBank` lets a join fall back to a bank
  another archive of the resident set carries.

### Migration

| Removed                    | Replacement      |
| -------------------------- | ---------------- |
| `bof3-emi` (Cargo package) | `rebof3-emi`     |
| `bof3_emi` (Rust path)     | `rebof3_emi`     |
| `emi ...` (command)        | `rebof3 emi ...` |

## 0.1.0

Initial EMI container library: the entry table, packing and `emi.json` manifests, LZ77, area
bundles, type-8 cue tables, classification and coverage.
