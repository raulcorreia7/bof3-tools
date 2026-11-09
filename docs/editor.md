# Asset editor coverage

What `bof3-edit` offers for each EMI entry format that `rebof3_formats::classify` knows. The row is
finished when it has a view, an edit where the format allows one, a byte-exact save, and a test that
proves the round trip, or a stated reason why one of them is missing. The views are listed in the
[README](../README.md#asset-editor).

Every format has the Info and Hex views. Hex edits any byte, and every edit goes through the
project, so saving writes `edits/<archive>/<index>.bin` and **Export** rebuilds the archive and reads
it back. A move in the Layout view is saved as `edits/<archive>/<index>.load`, the entry's load
argument in hex. An entry no view changes keeps its stored bytes and its disc load argument.

## Game catalog

The left panel's **Game** tree files every entry of the disc once, the way the developers keep their
assets. `gui/rebof3-editor/src/catalog.rs` holds the rules; each cites its decomp symbol
(`include/sys/cd_format.h` `EMI_SLOT_*`, `EMI_FAMILY_*`) or format doc. An entry goes to the first match:

1. A fixed game table (`ETC/GAME.EMI` entry 0, `ETC/START.EMI` entry 8, found by the editor's table
   sheets) goes to **Data tables**; the line names the sheets and, for the party, the characters.
2. Its format, when the archive that holds it is not its owner: encounter tables to Enemies and bosses
   (formations), auxiliary windows to Interface, shared resource tables to Text.
3. Its archive's family: `PL`, `PLP`, `BPLD`, `BPLU` and the other `BPLCHAR` banks to Characters;
   `ENEMY` and `BOSS` to Enemies and bosses; `AREA` to Areas and maps; the `BATTLE` block, `MAGIC` and
   `KAIZAR` to Battle; `START`, `STATUS`, `SHOP`, `FIRST`, `ENDKANJI` and `GAME` to Interface; `BGM*` and
   the area sound banks to Audio; `DEMO`, `SCENA*` and the ETC programs to Movies and other.
4. Otherwise **Unsorted**, with the reason as the section title (`MTEST` and `RTEST` have no caller in
   the decomp).

Archives sort in the disc's order, which is the order of the slot ids, and numbered series count up.
Area message banks stay with their area. The digits of a character bank's name (`PL012`) are read as
`CharacterId` values (`include/game/character.h`) to name its party; the name is the only evidence, so
the catalog does not claim more than the title. The retail test `tests/catalog.rs` pins 6,344 entries in
880 archives, each filed once.

## Formats

The counts are the US disc's 6,344 entries in 880 archives. "Retail" tests skip without
`BOF3_DISC_DIR` (and fail under `BOF3_REQUIRE_RETAIL=1`).

| Format | Entries | View | Edit | Round-trip test | Gap or reason |
| ------ | ------: | ---- | ---- | --------------- | ------------- |
| VRAM texture upload (type 3) | 593 | Texture: 4, 8 or 16 bpp with a row a rule joins in the scene that loads it, a ranked suggestion, or a chosen row; VRAM: the scene's whole frame buffer, each region labelled by its entry, any region decoded at any depth with any CLUT row of the scene, exported as PNG | paint texels | retail: words back to the payload, 593/593; every upload of an archive a default scene loads shows in VRAM, except FIRST entry 4 | 202 of 593 are joined; the rest show a suggestion, never applied. FIRST entry 12 covers all of entry 4. `ENDKANJI` and `SCENA17` (4 uploads) have no default scene: their loaders are not read |
| CLUT bank | 620 | Palette, Decoded; VRAM: where the scene's palette upload puts it | colour channels and STP bit | retail: an edit rewrites only its colour word, 620/620 banks | none |
| VRAM layout (type 3 and CLUT bank) | 593 + 620 | Layout: the scene's texture and palette placements as labelled rectangles in the frame buffer, with each entry's depth, the draw sites that read it and its collisions in red | move a texture by column and row (its load argument), or a bank by its palette-arena address; back to the disc's value | retail: AREA024 lays out, and a move and its reverse rebuild the archive byte for byte; synthetic: moves, undo, save and rebuild | a placement the game sets in code cannot move and is shown grey with its reason: palette words of a compressed bank, uploads by named code. Draw sites in the executable and overlays name pages and rows as literals; a move changes the data only |
| VAB header (type 6) | 1,020 | Sound: samples, programs and tones; Tables: bank, programs, tones | master volume and pan, every program and tone field | retail: every sheet rewrites its bytes | none; the counts and sizes that lay out the header are not editable |
| VAB body (type 7) | 1,020 | Sound: play a sample as the audio library auditions it; play a tone alone or its program at a chosen note, held one pass of the sample (1 to 20 s) then released; all through the library's sound driver and SPU, in stereo at 44.1 kHz; export the raw sample as WAVE, and SoundFont | replace a sample from a mono 16-bit 44.1 kHz WAVE file; the header's size table and bank size follow, in one undo step | retail: every sample put back in place keeps the header and body, 8,330/8,330; every sample and tone of `BATL_SE.EMI` plays in stereo at 44.1 kHz | none; ADPCM is lossy, so a WAVE export imported back is close, not exact |
| SEP sequence package (type 10) | 119 | Sound: play with the archive's bank through the library's sound driver, in stereo, export MIDI | each sequence's resolution, tempo and time signature; import a sequence from MIDI | retail: playback; every sequence's MIDI imports back to its bytes, 476/476 | none |
| Character sprite bank, LZ77 (type 1) | 65 | Sprite bank: each section's animations played at the game's tick (33.4 ms) with play, pause, tick, step, restart, loop, once and reverse; each frame stitched through the VRAM model with its cells, frame box and actor position outlined; the frame sheet with the cells it cuts; the steps with their times and jumps; the palettes | palette colours; import a PNG sprite sheet with Aseprite JSON, exported from the view or `bof3 assets export sprite-sheet` | retail: every bank decodes and encodes to its bytes; every section's sheet imports back to the stored section, 273/273; a colour edit keeps the rest of the bank, 141/141 | a battle bank's reader is not traced: it is drawn with the field placement, and its palettes are a ranked suggestion drawn in grey until one is picked. Cell texels past the sheet and indices past the palette depend on the running game's VRAM and are counted, not drawn |
| Character sprite bank, raw | 79 | Sprite bank, as above | as above | retail, as above | as above; 3 entries are not sprite banks and show why |
| Area message bank | 200 | Text: records, message-window preview | rewrite a string in place | retail: every string decodes and encodes back | none |
| Area cell grid | 191 | Cell grid over the map size | paint a cell's nibble | retail: a repaint keeps the payload | none; the values have no names yet |
| Area script bank | 200 | Script: each section's records, each record's ops (`show` a frame for its ticks, `jump` to a labelled step, a click on the label picks the step), its frames' cells and its padding; Decoded tree; export and import the bank as text | every op operand and cell field in place; a step added or removed at the end of a stream, with its jumps checked | retail: the ops encode to the stored bytes and the text imports to them, 200/200; an operand edit rewrites one byte and undone gives the stored bytes, 279/279 banks of the three formats | a record is an animation stream (`Actor_SeekScript`), not event bytecode. Steps change at the end of a stream only, because game code names stream positions; no edit may grow the bank past its stored size |
| Battle script bank | 42 | Script and Decoded, as the area script bank | as the area script bank | retail: as the area script bank, 42/42 | as the area script bank; 84 shows name a frame past their frame table, whose cells the game reads from past the bank |
| Area actor definitions | 200 | Tables, Decoded | every sheet cell | retail: every sheet rewrites its bytes | none |
| Area encounters and enemies | 200 | Tables: encounters, enemies, enemy AI | every sheet cell | retail: every sheet rewrites its bytes | none |
| Area map bundle | 200 | Map: cells textured from the area's replayed VRAM, or by tile bank and height | corner heights | retail: decode and encode, 200/200; all 688,118 cells with a tile find their page and CLUT row | none; drawn flat, without the vertex colour or semi-transparency |
| Area auxiliary module window | 9 | Info: module id and body size | Hex only | n/a | area049's window is code, changed in the decompilation; the other eight hold data whose layout is not traced |
| Shared resource table | 44 | Text, Info: id groups | strings in place | retail: strings | none |
| Type-8 cue table | 904 | Tables, Decoded | every sheet cell | retail: every sheet rewrites its bytes | none |
| Magic-effect script bank (`0x800C3800`) | 37 | Script and Decoded, as the area script bank, with one section | as the area script bank | retail: as the area script bank, 37/37 | as the area script bank; 23 records are empty and show as raw |
| Code image | 548 | Info and Hex; `ETC/GAME.EMI` entry 0 also Tables | Hex; the game tables | retail: sheets | code is changed in the decompilation and rebuilt, not here |
| Reserve (all-zero payload) | 53 | Info and Hex | Hex only | n/a | the payload carries no data; the loader only reserves the space |
| Unclassified | 0 | Info and Hex | Hex only | n/a | no retail entry; types 2 and 9 never occur on the disc |

## Shell

| Feature | State | Gap |
| ------- | ----- | --- |
| Undo and redo | every view, through the project; a drag, a typed field or an import is one step, and undo closes an open field | none |
| Unsaved edits | the status line says `unsaved`; closing the project or the window asks to save or discard them | none |
| Malformed entries | each view reports a decoder error | none; a test draws every view of every format over an empty, a short and a noise payload |

Playback opens the output device at the library's 44.1 kHz when the device offers it. A device that
does not gets the clip resampled with [rubato](https://crates.io/crates/rubato)'s FFT resampler, a
band-limited sinc filter.

Not EMI entries, so out of scope: XA audio, STR video and CD-DA tracks. The `bof3` command line
handles them.
