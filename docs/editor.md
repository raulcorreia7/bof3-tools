# Asset editor coverage

What `bof3-edit` offers for each EMI entry format that `rebof3_formats::classify` knows. The row is
finished when it has a view, an edit where the format allows one, a byte-exact save, and a test that
proves the round trip, or a stated reason why one of them is missing.

Every format has the Info and Hex views. Hex edits any byte, and every edit goes through the
project, so saving writes `edits/<archive>/<index>.bin` and **Export** rebuilds the archive and reads
it back. A move in the Layout view is saved as `edits/<archive>/<index>.load`, the entry's load
argument in hex. An entry no view changes keeps its stored bytes and its disc load argument.

## Run and views

`bof3-edit` is a desktop editor for the disc's EMI archives. It runs on Linux (X11 and Wayland), macOS and
Windows.

```sh
bin/bof3-edit build/disc my-mod
```

The first argument is an extracted disc (the folder that holds `BIN/`), the second a project folder. The
editor only reads the disc. Edits are saved to `my-mod/edits/`, and **Project → Export** rebuilds each
edited archive into `my-mod/disc/`, then reads it back to confirm that it holds exactly the edited
payloads.

The left panel has two trees, switched at its top (**Game** and **Disc**) and remembered in the project
folder. **Game** is the catalog: characters, enemies and bosses, areas and maps, battle, interface,
audio, text, data tables, movies and other, each with its entry count, in the game's own order (area
number, enemy bank, boss, spell effect). Every entry of every archive is in it once, and a name no rule
places is under Unsorted with the reason. **Disc** is the file tree. The filter works in both, and
each line opens the views below. [Game catalog](#game-catalog) says how the catalog files an entry.

| View    | Entries                       | What you can do                                                    |
| ------- | ----------------------------- | ------------------------------------------------------------------ |
| Info    | every entry                   | type, load address, size, format and what is known about it        |
| Hex     | every entry                   | edit bytes; read and write the value at the cursor as any int width |
| Texture | VRAM uploads                  | view at 4, 8 or 16 bits with a palette row a draw rule joins in the scene that loads it, or pick one; a ranked depth and row suggestion when no rule joins; paint |
| VRAM    | VRAM uploads, palette banks   | the whole 1024×512 VRAM of a field, world-map, menu, battle or effect scene, replayed from its archives; each region outlined with the entry that wrote it; decode any region at any depth with any CLUT row; export PNG |
| Palette | palette banks                 | edit any color's 5-bit channels and semi-transparency bit          |
| Tables  | `ETC/GAME.EMI` entry 0, `ETC/START.EMI` entry 8, area encounter blocks, actor definitions, cue tables, VAB headers | spreadsheets of items, equipment, shops, abilities, level growth, party members, encounters, enemies and their AI blocks, actor definitions, sound cues, and a sound bank's programs and tones; drops, equipment, skills and encounter slots show the name they point at |
| Text    | message banks, any strings    | rewrite a string in place (it must fit its old bytes); preview it as message windows with pages, lines and colours |
| Sound   | VAB banks, sequence packages  | play samples, tones at a chosen note and sequences through the audio library's SPU, in stereo (with the archive's own bank and your edits); export a raw sample as WAVE, SoundFont and MIDI; replace a sample from WAVE; edit a sequence's resolution, tempo and time signature; import a sequence from MIDI |
| Map     | area map bundles              | the map from above, textured from its area's replayed VRAM or coloured by tile bank and height; export the textured map as PNG; edit a cell's corner heights |
| 3D      | area map bundles              | the area as the field renderer draws it, through a free or the game's camera, drawn with OpenGL ES or in software; layers for exits, cell codes, the battle grid, encounter zones and actors; paint codes and battle values, raise cells, edit tile descriptors, move and turn actors, edit encounter rows; export and import glTF |
| Sprite bank | character sprite banks, raw or LZ77 | play each section's animations at the game's tick (loop, once, reverse; step by tick or step) with the frame stitched through VRAM and its cells outlined; the frame sheet; edit any palette colour; export and import a PNG sprite sheet with Aseprite JSON |
| Formation | area encounter blocks | each encounter's enemies placed as the field places them for battle, drawn through the area's VRAM and animated, for each scroll direction |
| Cell grid | area 4-bit cell grids       | the grid over the map's width and height; paint any cell's value   |
| Script  | area, battle and magic-effect script banks | each record's animation stream one op per line: `show` a frame for its ticks, `jump` to a labelled step; edit any operand or cell; add or remove a step at the end of a stream; export and import the bank as text |
| Decoded | cue, scene, area, battle and magic-effect script, palette, LZ77 | the decoder's structured reading |

Undo and redo cover every view (`Ctrl+Z`, `Ctrl+Y`); one undo takes back a whole drag or typed field. `Ctrl+S`
saves, `Ctrl+E` exports, and closing with unsaved edits asks to save or discard them. The editor
uses the system's fonts; set `BOF3_EDIT_FONT` (and `BOF3_EDIT_MONO`) to the name of an installed font
family to use another one.

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
| VRAM texture upload (type 3) | 593 | Texture: 4, 8 or 16 bpp with a row a rule joins in the scene that loads it, a ranked suggestion, or a chosen row; VRAM: the scene's whole frame buffer, each region labelled by its entry, any region decoded at any depth with any CLUT row of the scene, exported as PNG | paint texels | retail: words back to the payload, 593/593; every upload of an archive a default scene loads shows in VRAM, except FIRST entry 4 | 204 of 593 are joined, among them `BATL_DRA` entry 0 and `BATL_OVR` entry 1; the rest show a suggestion, never applied. FIRST entry 12 covers all of entry 4. `ENDKANJI` and `SCENA17` (4 uploads) have no default scene: their loaders are not read |
| CLUT bank | 620 | Palette, Decoded; VRAM: where the scene's palette upload puts it | colour channels and STP bit | retail: an edit rewrites only its colour word, 620/620 banks | none |
| VRAM layout (type 3 and CLUT bank) | 593 + 620 | Layout: the scene's texture and palette placements as labelled rectangles in the frame buffer, with each entry's depth, the draw sites that read it and its collisions in red | move a texture by column and row (its load argument), or a bank by its palette-arena address; back to the disc's value | retail: AREA024 lays out, and a move and its reverse rebuild the archive byte for byte; synthetic: moves, undo, save and rebuild | a placement the game sets in code cannot move and is shown grey with its reason: palette words of a compressed bank, uploads by named code. Draw sites in the executable and overlays name pages and rows as literals; a move changes the data only |
| VAB header (type 6) | 1,020 | Sound: samples, programs and tones; Tables: bank, programs, tones | master volume and pan, every program and tone field | retail: every sheet rewrites its bytes | none; the counts and sizes that lay out the header are not editable |
| VAB body (type 7) | 1,020 | Sound: play a sample as the audio library auditions it; play a tone alone or its program at a chosen note, held one pass of the sample (1 to 20 s) then released; all through the library's sound driver and SPU, in stereo at 44.1 kHz; export the raw sample as WAVE, and SoundFont | replace a sample from a mono 16-bit 44.1 kHz WAVE file; the header's size table and bank size follow, in one undo step | retail: every sample put back in place keeps the header and body, 8,330/8,330; every sample and tone of `BATL_SE.EMI` plays in stereo at 44.1 kHz | none; ADPCM is lossy, so a WAVE export imported back is close, not exact |
| SEP sequence package (type 10) | 119 | Sound: play with the archive's bank through the library's sound driver, in stereo, export MIDI | each sequence's resolution, tempo and time signature; import a sequence from MIDI | retail: playback; every sequence's MIDI imports back to its bytes, 476/476 | none |
| Character sprite bank, LZ77 (type 1) | 65 | Sprite bank: each section's animations played at the game's tick (33.4 ms) with play, pause, tick, step, restart, loop, once and reverse; each frame stitched through the VRAM model with its cells, frame box and actor position outlined; the frame sheet with the cells it cuts; the steps with their times and jumps; the palettes | palette colours; import a PNG sprite sheet with Aseprite JSON, exported from the view or `bof3 assets export sprite-sheet` | retail: every bank decodes and encodes to its bytes; every section's sheet imports back to the stored section, 273/273; a colour edit keeps the rest of the bank, 141/141 | a battle bank (`BPLD`, `BPLU`) is placed by `Party_LoadMemberSprite` through `Party_BuildBattlers(PARTY_ORDER_BATTLE)` and drawn with palette 0, as a field bank; the battle slot, a run-time choice, moves the sheet row and CLUT, not the colours. 38 battle and 19 field entries (171 sections, 33,202 frames) draw with a proven palette. The 83 form banks of `BPLCHAR` (`DRG`, `RYU`, `REI`, `PAPY`, `BRT`, `CRYU`, `RT`), which `func_801E7948` (`emi/battle/battle/03`) streams over section 0 by the tables `D_800B4A64` and `D_800B4A68`, and `ETC/START.EMI` entry 7 keep the grey drawing and the ranked suggestion: which section the battler's `+0x4C` names after the stream is not traced. Cell texels past the sheet and indices past the palette depend on the running game's VRAM and are counted, not drawn |
| Character sprite bank, raw | 79 | Sprite bank, as above | as above | retail, as above | as above; 3 entries are not sprite banks and show why |
| Area message bank | 200 | Text: records, message-window preview; Compose: each message in the game's font and window frame, page by page, with the choice panel, the window's placement from `{BOX}` and warnings for text past the window, lines below it and glyphs the font lacks | rewrite a string in place; Compose rewrites a message and rebuilds the bank, refused for text that does not encode, an empty message or a bank past `0xA000` bytes | retail: 165 banks, 10,510 messages and 18,540 pages lay out and render; every bank rebuilds from its messages and from its PO catalogue, 165/165; every drawn glyph but byte `FF` has a picture in the font | text a name or variable code reads at run time (`{INSERT}`, `{GOTO}`, `{VAR}`, `{LINK}`) is drawn as a placeholder as wide as the game could print it; `{OFFSET}` text is drawn at its cursor, not its animated place; no portrait, because no message code selects a face sheet; the window is drawn over black, not the field |
| Area cell grid | 191 | Cell grid over the map size | paint a cell's nibble | retail: a repaint keeps the payload | none; the values have no names yet |
| Area script bank | 200 | Script: each section's records, each record's ops (`show` a frame for its ticks, `jump` to a labelled step, a click on the label picks the step), its frames' cells and its padding; Decoded tree; export and import the bank as text | every op operand and cell field in place; a step added or removed at the end of a stream, with its jumps checked | retail: the ops encode to the stored bytes and the text imports to them, 200/200; an operand edit rewrites one byte and undone gives the stored bytes, 279/279 banks of the three formats | a record is an animation stream (`Actor_SeekScript`), not event bytecode. Steps change at the end of a stream only, because game code names stream positions; no edit may grow the bank past its stored size |
| Battle script bank | 42 | Script and Decoded, as the area script bank | as the area script bank | retail: as the area script bank, 42/42 | as the area script bank; 84 shows name a frame past their frame table, whose cells the game reads from past the bank |
| Area actor definitions | 200 | Tables, Decoded | every sheet cell | retail: every sheet rewrites its bytes | none |
| Area encounters and enemies | 200 | Tables: encounters, enemies, enemy AI; Formation: each encounter's enemies at the cells `Area_PlaceEncounterEnemies` gives them for a chosen scroll direction, drawn from the scene record `Battle_InitEnemy` looks up through the area's replayed VRAM and played at the game's tick | every sheet cell | retail: every sheet rewrites its bytes; 599 formations place their 1,872 enemies, 1,780 find a scene record and a stream, and all 16,322 frames of those streams draw with no texel past an upload | the spawn roll, a blocked cell and the record the battle plays (`func_801E2314`) are run-time choices: every slot is kept, the first cell is shown and the script entry is picked; 92 enemies name a scene id the area's table lacks |
| Area map bundle | 200 | Map: cells textured from the area's replayed VRAM, or by tile bank and height; 3D: the terrain with its walls, grey levels and blend modes (`func_801530D0`, `func_80155560`) through a free or the game's camera, with layers for exits, cell codes, the battle grid, encounter zones and actor markers, each record and its reader shown under the pointer | corner heights, raise and lower, cell codes, battle grid values, descriptor fields (shared by every cell that names the descriptor); an actor's cell, facing and scene id in the area program; encounter rows; export glTF with `cells`, `actors` and `encounters` nodes and vertex colours, import its heights and actor places | retail: decode and encode, 200/200; all 688,118 cells with a tile find their page and CLUT row; 200/200 areas read, draw and texture in 3D, 5,280 actors, 666 zone rows; 32,220 unchanged edits rebuild byte for byte; 200/200 glTF exports import back byte for byte; a code and a facing edit change one byte each and undo restores them | actors are markers: the field sprite draw (`Area_DrawObjectSprite`) is not decoded. Encounter zones are in `SLUS_004.22`, which the project does not rebuild, so they are read only. Exit targets (`AreaLoad` +0x20) are not decoded. A position stays inside the map and a scene id must be a row of the area's scene table |
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
