# Asset output

`bof3 assets extract` writes every asset twice: the bytes the disc stores, in a file named by the
format they are in, and the view a human opens. The commands are in [the command line](cli.md#assets);
the formats are in the [wiki](https://raulcorreia7.github.io/bof3-wiki/formats/assets.html).

## Output layout

The original file is named by the real format: the game's VRAM upload is `img`, its CLUT bank `clut`,
its map bundle `map`, the two halves of a sound bank `vh` and `vb` (the whole bank is `vab`), a
sample's waveform data `vag`, a sequence package `seq`, an LZ77 blob `lz77`, and a type-8 cue table
`emi8`. Other entries carry the EMI type tag `emi0`. `assets.jsonl` records the source and the SHA-256
of every file.

| Kind              | Original file                                                      | Viewable file                                                             |
| ----------------- | ------------------------------------------------------------------ | ------------------------------------------------------------------------- |
| `texture`         | `texture/<disc path>/<slot>.img`                                   | `<slot>.png` (16 bpp), `<slot>.clut-<x>-<y>.png` (an indexed row), `.tim` |
| `palette`         | `palette/<disc path>/<slot>.clut`                                  | `<slot>.json`, `<slot>.png` (swatch strip)                                |
| `audio-bank`      | `<slot>.vh`, `<slot>.vb`, `<slot>.vab`, `<sample>.vag`             | `<slot>.sf2`, `<sample>.wav`                                              |
| `audio-sequence`  | `<slot>.seq`                                                       | `<music>.mid`                                                             |
| `audio-cues`      | `<slot>.emi8`                                                      | `<slot>.json`                                                             |
| `area-map`        | `<slot>.map`                                                       | `<slot>.obj`, `.mtl`, `.glb`, `.layout.json`                              |
| `area-actor-defs` | `<slot>.emi0`                                                      | `<slot>.json`                                                             |
| `area-encounters` | `<slot>.emi0`                                                      | `<slot>.json`                                                             |
| `area-script`     | `<slot>.emi0`                                                      | `<slot>.json`, `<slot>.txt` (the ops)                                     |
| `battle-script`   | `<slot>.emi0`                                                      | `<slot>.json`, `<slot>.txt` (the ops)                                     |
| `bmagic-table`    | `<slot>.emi0`                                                      | `<slot>.json`, `<slot>.txt` (the ops)                                     |
| `character-bank`  | `<slot>.lz77` or `<slot>.emi0`                                     | the decoded blob `<slot>.bin` of an LZ77 bank                             |
| `cdda`            | `cdda/track<NN>.pcm` (16-bit stereo, 44.1 kHz, no header)          | `track<NN>.wav`                                                           |
| `data-table`      | `data-table/BIN/ETC/GAME/<name>.bin` (the table's span of entry 0) | `<name>.json`                                                             |
| `xa-audio`        | the `.STR` file as stored                                          | `.wav`                                                                    |
| `text`            | `text/<disc path>/<slot>.bin`                                      | `<slot>.json`, `<slot>.txt`, `text/index.jsonl`                           |

`assets kinds` lists every kind with its handling: `decoded` (stored bytes and a derived file), `raw`
(stored bytes only) or `none`.

The `.vab` file is the sound bank as one file: the header followed by the body. The `.vag` file holds a
sample's waveform data as the VAB body stores it; the 48-byte header of a standalone VAG file is not
written, because the rate and the loop window of a sample live in the bank's tone table. The `.tim`
file is the standard PS1 TIM holding the texture's words and the palette row's color words; the game
stores no TIM of its own.

## Modes

`--mode raw` writes the bytes the disc stores. `--mode modern` writes the conversion to an open format.
No flag writes both. A kind with no conversion has nothing to add in `modern`, so its stored bytes are
its modern output too.

| Kind                                                                                                                                               | `--mode raw`                 | `--mode modern`                                    |
| -------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------- | -------------------------------------------------- |
| `texture`                                                                                                                                          | `.img`                       | `.png`, `.tim`                                     |
| `palette`                                                                                                                                          | `.clut`                      | `.json`, `.png`                                    |
| `audio-bank`                                                                                                                                       | `.vh`, `.vb`, `.vab`, `.vag` | `.sf2`, `.wav`                                     |
| `audio-sequence`                                                                                                                                   | `.seq`                       | `.mid`                                             |
| `audio-cues`, `area-actor-defs`, `area-encounters`                                                                                                 | `.emi8`, `.emi0`             | `.json`                                            |
| `area-script`, `battle-script`, `bmagic-table`                                                                                                     | `.emi0`                      | `.json`, `.txt`                                    |
| `character-bank`                                                                                                                                   | `.lz77` or `.emi0`           | `.bin` for an LZ77 bank; an unpacked bank has none |
| `area-map`                                                                                                                                         | `.map`                       | `.obj`, `.mtl`, `.glb`, `.layout.json`             |
| `cdda`                                                                                                                                             | `.pcm`                       | `.wav`                                             |
| `data-table`                                                                                                                                       | `.bin`                       | `.json`                                            |
| `xa-audio`                                                                                                                                         | the `.STR` file              | `.wav`                                             |
| `executable`, `system`, `video`, `area-message`, `area-cells`, `area-module`, `resource-table`, `code`, `unknown`                                  | the stored bytes             | the stored bytes                                   |
| `text`                                                                                                                                             | `<slot>.bin`                 | `.json`, `.txt`, `index.jsonl`                     |

## Catalog

`assets.jsonl` has one object per output file with the keys `source`, `slot`, `offset`, `size`,
`type`, `load_arg`, `format`, `kind`, `state`, `role`, `output`, `bytes`, `sha256`, `evidence` and
`note`. `role` is `raw` for the stored bytes (`vab` and `vag` are also the disc's own bytes); a derived
file names its format (`png`, `tim`, `json`, `obj`, `mtl`, `glb`, `sf2`, `wav` or `midi`; `script` for a script
bank's ops as text). `kind` is the
asset kind and `format` is the entry's format. `evidence` names the loader evidence behind a derived
file. A derived file of one entry carries the same `source` and `slot` as that entry.

## Text

For every payload that holds text, the `text` kind writes three files below
`text/<disc path without .EMI>/`:

| File          | Contents                                                         |
| ------------- | ---------------------------------------------------------------- |
| `<slot>.bin`  | the entry's stored bytes (the message bank or name table)        |
| `<slot>.json` | one row per string, with its source chain                        |
| `<slot>.txt`  | one line per string: `offset<TAB>id<TAB>text`, the offset in hex |

`text/index.jsonl` indexes the run with `source`, `kind`, `id`, `ram` and `text`. Its `source` is
`<disc path>#<slot>+<offset>`, so a line found with `grep` names the bytes that produced it. The scan
keeps a payload only when it holds text; `--list` prints each payload it drops. The string
encoding and the tables are in the [text page](https://raulcorreia7.github.io/bof3-wiki/data/text.html).

## Import

Export and import are paired. Import takes the stored payload as `--payload` (or `--base`) and keeps
every byte the open form does not name.

- **Texture.** `export texture PAYLOAD --load-arg HEX` writes the view; `--depth 4|8 --palette BANK
  --palette-offset WORDS` writes an indexed row. `import texture PNG [-o OUT] [--palette OUT]` reads a PNG
  back to the stored payload; an indexed PNG also gives its color words, which `--palette` writes as
  raw little-endian words.
- **Palette.** `import palette` reads the `word` of each color. The `rgba` and `stp` fields are views and
  are not read.
- **Sound bank.** `soundfont HEADER BODY` writes a SoundFont 2 file. `import soundfont` reads it back to
  the VAB header and body: each preset of bank 0 becomes a program, each zone a tone, and each sample
  a VAG from the ADPCM encoder. Keys, root note, tuning, level, pan, loop and the volume envelope are
  kept; the envelope becomes the ADSR registers whose timing matches (the nearest pair when none
  matches). A loop ends at the last frame of its sample, and frames past it are cut. Priority,
  reverb, vibrato, portamento, pitch bend, the program's volume and pan, and the bank id take their
  defaults. The samples are ADPCM again, so no bank comes back byte for byte; an import of an export of
  an imported bank gives that bank again.
- **Sample.** `export vag HEADER BODY --index N` writes a mono 16-bit WAVE file at 44.1 kHz.
  `import vag` encodes such a file back to a VAG. The encoder picks its own filter and shift for each
  28-sample block, so a stored VAG does not return byte for byte; a second round trip is exact. The
  import sets no loop flags, because a WAVE file carries none.
- **Sequence.** `export sequence PACKAGE --index N` writes MIDI; `import sequence FILE.mid:ID...` writes a
  package.
- **Game tables.** `tables PAYLOAD -o DIR` writes one JSON file per table of `ETC/GAME.EMI` entry 0.
  `import table NAME JSON --payload ENTRY` writes one table's span of bytes. A name keeps its stored
  bytes while its text is unchanged, because the text drops color codes; a changed name is written in
  the table encoding, and a backslash is written as `\x5C`. A growth stat must be a nibble (0 to 15).
  Splice the output into entry 0 at the table's offset and use `import emi --entry 0=FILE`.
- **Maps.** `terrain` and `export terrain` write glTF or OBJ; `import terrain MESH --payload BUNDLE` and
  `import layout JSON --payload BUNDLE` rebuild the bundle. The glTF's mesh has one quad per cell:
  `x` is the column, `z` the row and `y` the corner byte times 16. Its `COLOR_0` is each cell's grey
  level over 255, where the GPU takes `0x80` as neutral, and a `cells` node holds every cell's byte
  code in its `extras`. The import reads the mesh's corner heights alone. `import encounters JSON --base BLOCK`
  rebuilds an encounter block.
- **Text and characters.** `export message`, `export strings`, `export characters` and
  `export character-bank` pair with the `import` verbs of the same name. `character-bank` imports palette
  tables only.
- **Script banks.** `export script PAYLOAD` writes an area, battle or magic-effect script bank as text,
  one op per line; `import script TEXT --payload PAYLOAD` reads it back. A record is the animation
  stream `Actor_SeekScript` (`exe/slus_004_22@0x8014D910`) plays: `show frame F ticks T` stages a
  frame for its ticks (`Actor_StageAnimStep`), and `jump LABEL` continues at a labelled step (`Actor_StepAnimLoop`,
  `Actor_StepAnimOnce`); every first byte selects one of the two. `frame` and `cell` lines give the frames,
  `pad` the bytes before the next record, and `raw` a record that does not read as a stream. The
  text carries every byte, so an unchanged export imports to the stored bank. A jump names a label, so a
  step added in the text moves the jumps after it; the import refuses a bank larger than `--payload`,
  because what follows the bank in memory is not known to be free. Game code names stream positions
  (`Actor_BeginScript`'s start position, `script_pc` tests), and the import cannot check those.
- **Sprite sheets.** `export sprite-sheet PAYLOAD --exe SLUS_004.22 --section N --palette N -o OUT.png`
  writes one section of a sprite bank as an 8-bit indexed PNG and `OUT.json` in Aseprite's array form:
  one row per animation a record plays, one cell per frame stitched through the VRAM model, one frame
  per step with the step's time in milliseconds (a tick is two NTSC fields, 33.4 ms), and one tag per
  animation. `meta.bof3` holds the section, palette, actor position, loop points and evidence. The cell
  sizes come from `g_SpriteSizes` in the executable. `import sprite-sheet OUT.json --payload PAYLOAD
  --exe SLUS_004.22` reads the PNG named in the JSON from the same folder and writes the payload back,
  raw or LZ77 as stored. A texel, colour or duration comes back only when it changed, so an unchanged
  sheet gives the stored payload byte for byte; two different changes to one shared texel, paint
  outside every cell, and an edit that outgrows the section's padding are refused. The editor's Sprite
  bank view writes and reads the same files.
- **Archives and discs.** `import emi CONTAINER --entry INDEX=FILE` keeps every entry's type and load
  address; a changed payload moves the entries after it. `import disc CUE --file PATH=FILE -o DIR` writes
  a patched copy of the disc; a replacement file cannot grow past its last sector.

Cue tables, area cells, actor definitions, XA and STR streams and CDDA tracks have no import verb. An import verb that does not exist for a kind fails with a usage error.

## Period files

A 1997 PlayStation studio kept its textures as TIM files from TIMTOOL, its sound banks as VAB, VH and
VB files from VABTOOL, and its sequences as SEQ files, or SEP files for several, from smf2seq. The
tools write and read these beside the modern formats. The layouts follow Sony's *File Formats*
manual (`FileFormat47.pdf`: TIM in chapter 2, VAB in chapter 3, SEQ and SEP in chapter 4) and
[psx-spx](https://psx-spx.consoledev.net/). A studio's tool reads the files by those layouts; this
repository does not run those tools.

- **TIM.** `export tim PAYLOAD --load-arg HEX` writes a type-3 texture as a TIM whose image block
  sits at the VRAM place the load argument names. A texture a draw site joins takes `--depth 4|8 --clut X,Y --palette BANK
  --palette-offset WORDS`: the TIM then has a CLUT block at `X,Y` and the matching 4 or 8 bpp image.
  Without `--depth` the TIM is 16 bpp with no CLUT. `--clut` is required with `--depth` and is
  never guessed. `import tim FILE [--load-arg HEX] [--palette OUT] [--bank BANK --palette-offset
  WORDS]` reads the image back to the stored payload, its place to a load argument, and the CLUT to
  the colour words; with `--bank` they are spliced into the bank. An image that is not on the
  32-word, 32-row grid the loader uploads in is refused. An unchanged export imports to the stored
  bytes. The extractor writes a plain 16 bpp `.tim` for a type-3 texture no row joins.
- **VAB.** `export vab HEADER BODY` writes one `.vab` file, the header followed by the body, as
  VABTOOL writes it; `--mode` still writes the `.vh` and `.vb` pair. `import vab FILE.vab -o OUT.vh
  --body OUT.vb` splits a file back. A pair is checked first: the header's size, the `pBAV` magic and
  the sample sizes against the body.
- **SEQ and SEP.** `export sep PACKAGE` writes the package as a SEP file; the stored `.seq` entry is
  already one. `export seq PACKAGE --index N` writes one sequence as a SEQ file. `import sep` and
  `import seq FILE.seq:ID...` check the file against the package rules and write a package. The SEQ
  version word is written as 1: the manual gives no value for it.

An unchanged file of each kind comes back byte for byte.

## Round-trip coverage

`assets coverage` reads every EMI entry on the disc and prints one line per kind: the entries and
bytes of the kind, and the part that exports and imports to its own bytes. It measures four kinds:
`area-message`, `area-encounters`, `data-table` (the game tables and the characters) and `text` (the
strings and the message banks together). A fifth line counts the palette tables of the sprite
banks against their decoded bytes. Every other kind prints `no round trip yet` and zero; the note
means the command does not measure the kind, not that the tool cannot write it back.

The other kinds round trip through their own commands. Results over the retail disc:

| Kind or path                                     | Result                                        | Verdict  |
| ------------------------------------------------ | --------------------------------------------- | -------- |
| EMI archive, `emi extract -J` then `emi pack -J` | 880 of 880 archives                           | lossless |
| EMI archive, `emi pack` without `-J`             | entry types and load addresses are lost       | lossy    |
| texture, 16 bpp view                             | every view reads back to the stored words     | lossless |
| texture, indexed PNG                             | all 882 joined rows                           | lossless |
| palette                                          | every palette reads back to its words         | lossless |
| texture through TIM                              | all 202 joined textures (882 rows); 593 uploads | lossless |
| sound bank through one VAB file or VH and VB     | 1,020 of 1,020 banks                          | lossless |
| sequences through SEP and SEQ                    | 119 packages, 476 sequences                   | lossless |
| sound bank through SoundFont                     | 0 of 1,020 banks come back to their own bytes | lossy    |
| sound bank with `--mode raw`                     | 1,020 of 1,020 banks                          | lossless |
| VAG sample through WAVE                          | no sample comes back byte for byte            | lossy    |
| sprite bank section through a sprite sheet       | 273 of 273 sections                           | lossless |
| script bank through its text                     | 279 of 279 banks                              | lossless |

`assets import vag` drops the end-marker block and the loop flags. The retail cases need the disc and
skip without it.
