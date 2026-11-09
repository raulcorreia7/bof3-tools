# ADR 0002: self-contained formats

- Status: accepted, partly implemented (see Implementation status)
- Date: 2026-11-09
- Requested by: raulcorreia7 (owner): "breakdown formats in an independent way, self contained"
- Applies to: `rebof3-emi`, `rebof3-assets`, `rebof3-formats` (new), `bof3`, `bof3-edit`

## Context

What the tools know about one format is spread over three crates:

| Where | What it holds for a format | Size |
| ----- | -------------------------- | ---: |
| `rebof3-emi` | the parser and encoder, the entry classifier, the coverage table | 5,699 lines in 22 format modules, plus 1,191 of container and primitives |
| `rebof3-assets` | the conversion to open files, the imports, the extract-all `match` over kinds | 3,257 lines of conversion, 1,697 of extraction, kinds, codec table and report |
| `rebof3-editor` | the view, the tab list, the colours, the schema | `Format::` is matched in 17 files |

Three costs follow.

- Adding or changing a format touches the classifier, the coverage `match`, the kind table, the
  codec table, the extractor and the editor's tab list. `classify.rs` alone had six `match`es over
  the same 21 variants (`name`, `state`, `spec_state`, `codec_state`, `evidence`, `extension`).
- `rebof3_emi::Error` lists the errors of every format (map, scene, script, encounter, text,
  character table, sprite bank), so the container crate knows the formats it carries.
- Format modules reach into each other: `encounter` and `characters` use `game_tables`, which uses
  the text codec; `texture` and `replay` use each other's constants; the coverage table names
  every format's codec.

A parallel lane that adds a format has no seam to add it at.

## Decision 1: the contract of a format module

Each format is one module with these parts. A part that a format cannot have (no encoder for the
LZ77 blob, no open form for the reserve) is absent, and the description says so.

| Part | Shape | Rule |
| ---- | ----- | ---- |
| `decode` / `encode` | `decode(&[u8]) -> Result<T>`, `T::encode(&self) -> Vec<u8>` | Identity on unchanged data: `encode(decode(b)) == b` for every retail payload. A payload that does not parse is an error, never a panic. |
| `export` / `import` | `export(&[u8]) -> Result<Vec<u8>>`, `import(&[u8], base) -> Result<Vec<u8>>` | The standard files: indexed PNG for textures and palettes, WAV for samples, SF2 for banks, MIDI for sequences, glTF and OBJ for terrain, JSON or CSV for tables and text. `import(export(b), b)` is `b`. |
| `describe` | a `Description` static | The game meaning and the evidence: the name, the selector that recognises the entry (type and load address), the extension, the documentation and codec state, the citation, and the round-trip check. |
| tests | in the module and in `tests/` | Synthetic cases always run. Retail cases read `BOF3_DISC_DIR` (the extracted disc) and `BOF3_DISC_IMAGE` (the cue sheet). Unset, a case skips and passes; set but invalid, it fails with the path; `BOF3_REQUIRE_RETAIL=1` (`make check-retail`) turns every skip into a failure. Each retail case asserts a pinned count. |

Dependency rules:

1. A format module depends on **primitives** only: the EMI container (`Entry`, `Archive`), LZ77,
   the record readers, the game text codec, `psx-formats`, and the VRAM model with its load replay.
   It never imports another format module. `tests/independence.rs` reads the sources and fails on
   a `crate::<other format>` path.
2. The EMI container (`rebof3-emi`) knows entries and the primitives: the table, the payload
   bytes, packing, manifests, LZ77, the record readers, the text codec, the VRAM model and the
   round trip of the container itself. It imports no format module. Its error keeps the variants
   for the character table, which the sprite lane still edits (see below).
3. A **registry** (`registry.rs` in `rebof3-formats`) lists every `Description` in one table. It
   answers "which format is this entry" (`classify`), "what is it called, how well is it known, what
   extension does it take" and "does its codec rebuild the bytes". The coverage table, the
   extractor and the editor ask the registry; none of them matches on a format to get a name or a
   state.
4. The extractor and the command line map a **kind** to its `export` and `import` through the
   registry's rows, and the editor maps a format to its **views** through one table in the editor
   (views are GUI code and stay in `rebof3-editor`, which depends on `rebof3-formats`; the reverse
   never holds).
5. Generic PlayStation formats (TIM, VAB, SEQ, SPU ADPCM, CLUT, VRAM words, PNG, WAV, SF2, MIDI)
   stay in `psx-formats` in `bof3-player`. A format module here adds only what the game does with
   them: the load argument, the pairing of a VAB header with its body, the join of a texture to a
   palette row.

### Error type

`rebof3-formats` has one `Error` for the parse and encode failures of the formats it holds. The
container keeps `InvalidArchive`, `EntryOutOfRange`, the cue errors (the `emi` crate defines them
for the audio library), LZ77 and I/O.

## Decision 2: layout, one crate with a module per format

```text
psx-formats (bof3-player)   TIM, VAB, SEP, CLUT, VRAM words, PNG, WAV, SF2, MIDI
        ^
rebof3-emi                  container, pack, manifest, verify, LZ77, record readers, text codec,
        ^                   VRAM model (texture) and load replay
rebof3-formats              registry + one module per format + shared glTF and identity helpers
        ^
rebof3-assets               extract-all, disc walk, kinds, report (calls the registry)
        ^
bof3, bof3-edit             command line; editor views (a table from format to views)
```

Chosen: **one crate, `rebof3-formats`, with a module per format**. Rejected: a crate per format.

| Fact | One crate | A crate per format (about 16) |
| ---- | --------- | ----------------------------- |
| Clean build today | `rebof3-emi` (all formats and container) 4.3 s wall, 27 s CPU; `rebof3-assets` 3.9 s wall, 48 s CPU, at `opt-level = 2` in the dev profile | Cargo builds independent crates in parallel, but each adds a rustc start, a metadata step and a link of its dependents; the workspace is already 4.3 s wide at the container, so parallel crates save little |
| Rebuild after one edit | Recompiles the formats crate: about 4 s | Recompiles the edited crate and the registry crate that names it: about the same, because the registry depends on all of them |
| Workspace entries | one manifest, one `[profile.dev.package.*]` override | 16 manifests, 16 overrides (the dev profile names each crate that decodes on the test path), 16 `[workspace.dependencies]` rows |
| Where a boundary is enforced | by a source test (rule 1); a module can still import another by mistake until the test fails | by Cargo: an import of another format does not compile |
| Reuse by an outside user | all or none | pick one format |

The only thing a crate per format buys is a compiler-enforced boundary, and nobody outside the
workspace uses these crates (`publish = false`). The source test costs one file and gives the same
guarantee in CI. The registry needs every format in one place anyway, so the dependency graph of a
crate per format is a star whose centre rebuilds on every edit. If a format needs a dependency the
others should not carry (an audio decoder, an image library), that format moves to its own crate
then; today every format uses `psx-formats`, `serde` and `serde_json` only.

`rebof3-emi` keeps its `no_std + alloc` mode for the container table and the cue table that the
audio library reads. `rebof3-assets` keeps what is not a format: the disc walk, the output tree,
the report, the kinds and the command-line codec table.

## Decision 3: how formats move

One format per commit, behaviour-preserving. Each commit keeps every test, the command-line output
(`cli/rebof3/tests/help.snap`), the editor behaviour and the byte-exact rebuilds, and ends with
`make check` and `make check-retail` (`BOF3_DISC_DIR` and `BOF3_DISC_IMAGE` set). The test count is compared before and after.

Order: the registry first (the classifier and the coverage table, which every later move reads),
then the formats that nothing else in the container uses, then the ones others depend on:

1. registry, selection rules, coverage (done in the first move)
2. the text codec and the record readers as primitives; then `game_tables`, `encounter` and the
   characters table, which stop sharing code and share the primitives
3. `script`, `scene`, `map` with the terrain and glTF export
4. `vram` with the replay and the texture joins, `clut`
5. `vab`, `sep`, `cue` with the sound, SoundFont, WAV and MIDI conversion
6. `sprite_bank` and the characters table, last: another branch edits them

## Consequences

- A new format is one module, one registry row, one kind and, if it has a view, one editor row.
- `Format::name`, `state`, `spec_state`, `codec_state`, `evidence` and `extension` read the
  registry row; the six `match`es are gone.
- Callers import from `rebof3_formats` instead of `rebof3_emi` for everything that is a format.
  `rebof3_emi::classify` and `rebof3_emi::coverage` are gone; the container round trip is
  `rebof3_emi::verify`.
- Risk: a move that changes an error text changes a command-line message. The help snapshot and the
  retail tests pin the common ones; a moved error keeps its `Display` text.
- Risk: the classifier is edited by the branch that adds the sprite-frames format. Its edit lands
  as a new row in `sprite_bank.rs` instead of a new arm in six functions.

## Implementation status

Every format moved into `rebof3-formats`, one commit per move:

- the registry with `classify`, the coverage table and each format's `kind`, `derive` and `open`
  hooks, which replace the `match`es in `Format`, `Kind::of_format`, `decode::derive` and the
  per-format branches of `extract.rs`;
- `script`, `scene`, `map` (terrain, layout and glTF), `game_tables` and `encounter`;
- `vab` (with `vag` and `soundfont`) and `sep` (with `midi`);
- `clut` and `vram`, with the palette and texture PNG, TIM and JSON conversion and their imports;
- `message` (the bank and its JSON) and the shared `text` module (scan rules, strings, known
  tables, string sources);
- `sprite` (frame sheets, animations, records, draw), `character_bank` and `characters`, after the
  sprite-frames lane merged, with `GameTableShort`, `CharacterTable`, `CharacterBank` and
  `SpriteBank` moving from the container error to the formats error;
- description-only rows: `cells`, `window`, `resources`, `residue`.

`joins` (a texture's palette rows over a whole archive) and `text` read several formats, so they
are not formats. No format module imports them except `message`, which uses `text`; the
independence test lists `text` as shared. SoundFont fits in `vab` because `bof3player` depends only
on `psx-formats` and its own `emi` crate, so there is no cycle; the crate takes it for the SPU
envelope steps.

The editor maps a format to its views through one table (`VIEWS` in `view/mod.rs`).

Left:

- `rebof3-emi` keeps only the container and primitives (LZ77, LZSS, record readers, text codec, VRAM
  model and replay), and its error has no variant for a format.
- `rebof3-assets` keeps the disc walk, the output tree, the report, the kinds, the CLI codec table,
  the sound rendering and the text extraction run, which are not formats.
- `Kind::of_format` reads the row's `kind` name; the kinds themselves stay in `rebof3-assets`.
