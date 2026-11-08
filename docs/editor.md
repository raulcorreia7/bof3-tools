# Asset editor coverage

What `bof3-edit` offers for each EMI entry format that `rebof3_emi::classify` knows. The row is
finished when it has a view, an edit where the format allows one, a byte-exact save, and a test that
proves the round trip, or a stated reason why one of them is missing. The views are listed in the
[README](../README.md#asset-editor).

Every format has the Info and Hex views. Hex edits any byte, and every edit goes through the
project, so saving writes `edits/<archive>/<index>.bin` and **Export** rebuilds the archive and reads
it back. An entry no view changes keeps its stored bytes.

## Formats

The counts are the US disc's 6,344 entries in 880 archives. "Retail" tests skip without
`BOF3_DISC_DIR`.

| Format | Entries | View | Edit | Round-trip test | Gap or reason |
| ------ | ------: | ---- | ---- | --------------- | ------------- |
| VRAM texture upload (type 3) | 593 | Texture: 4, 8 or 16 bpp with a joined or chosen palette | paint texels | retail: words back to the payload, 593/593 | none |
| CLUT bank | 620 | Palette, Decoded | colour channels and STP bit | none | needs a test |
| VAB header (type 6) | 1,020 | Sound: samples, programs and tones | Hex only | none | the program and tone tables are fixed records: needs sheets |
| VAB body (type 7) | 1,020 | Sound: play, export WAVE and SoundFont | Hex only | none | needs a sample import |
| SEP sequence package (type 10) | 119 | Sound: play with the archive's bank, export MIDI | Hex only | retail: playback | needs sequence header edits and MIDI import |
| Character sprite bank, LZ77 (type 1) | 65 | Sprite bank: node tables and palettes | palette colours; re-encoded on an edit | retail: a colour edit keeps the decoded bank | none; the node pixel format is not decoded |
| Character sprite bank, raw | 79 | Sprite bank | palette colours | retail, as above | none; 3 banks have no palette layer |
| Area message bank | 200 | Text: records, message-window preview | rewrite a string in place | retail: every string decodes and encodes back | none |
| Area cell grid | 191 | Cell grid over the map size | paint a cell's nibble | retail: a repaint keeps the payload | none; the values have no names yet |
| Area script bank | 200 | Decoded tree | Hex only | none | the bytecode is not decoded, so records are not editable as values |
| Battle script bank | 42 | Decoded tree | Hex only | none | as the area script bank |
| Area actor definitions | 200 | Tables, Decoded | every sheet cell | retail: every sheet rewrites its bytes | none |
| Area encounters and enemies | 200 | Tables: encounters, enemies, enemy AI | every sheet cell | retail: every sheet rewrites its bytes | none |
| Area map bundle | 200 | Map: cells by tile bank and height | corner heights | retail: decode and encode, 200/200 | none; textures need the tile page, not traced |
| Area auxiliary module window | 9 | Info and Hex | Hex only | none | needs a view |
| Shared resource table | 44 | Text, Info: id groups | strings in place | retail: strings | none |
| Type-8 cue table | 904 | Tables, Decoded | every sheet cell | retail: every sheet rewrites its bytes | none |
| Magic-effect table (`0x800C3800`) | 37 | Info and Hex | Hex only | none | needs a view |
| Code image | 548 | Info and Hex; `ETC/GAME.EMI` entry 0 also Tables | Hex; the game tables | retail: sheets | code is changed in the decompilation and rebuilt, not here |
| Reserve (all-zero payload) | 53 | Info and Hex | Hex only | n/a | the payload carries no data; the loader only reserves the space |
| Unclassified | 0 | Info and Hex | Hex only | n/a | no retail entry; types 2 and 9 never occur on the disc |

## Shell

| Feature | State | Gap |
| ------- | ----- | --- |
| Undo and redo | every view, through the project | a drag or a typed field is one step per change, not per gesture |
| Unsaved edits | the status line says `unsaved` | closing the project or the window drops them without asking |
| Malformed entries | each view reports a decoder error | needs a test that feeds every view's reader a bad payload |

Not EMI entries, so out of scope: XA audio, STR video and CD-DA tracks. The `bof3` command line
handles them.
