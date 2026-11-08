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
| CLUT bank | 620 | Palette, Decoded | colour channels and STP bit | retail: an edit rewrites only its colour word, 620/620 banks | none |
| VAB header (type 6) | 1,020 | Sound: samples, programs and tones; Tables: bank, programs, tones | master volume and pan, every program and tone field | retail: every sheet rewrites its bytes | none; the counts and sizes that lay out the header are not editable |
| VAB body (type 7) | 1,020 | Sound: play, export WAVE and SoundFont | replace a sample from a mono 16-bit 44.1 kHz WAVE file; the header's size table and bank size follow, in one undo step | retail: every sample put back in place keeps the header and body, 8,330/8,330 | none; ADPCM is lossy, so a WAVE export imported back is close, not exact |
| SEP sequence package (type 10) | 119 | Sound: play with the archive's bank, export MIDI | each sequence's resolution, tempo and time signature; import a sequence from MIDI | retail: playback; every sequence's MIDI imports back to its bytes, 476/476 | none |
| Character sprite bank, LZ77 (type 1) | 65 | Sprite bank: node tables and palettes | palette colours; re-encoded on an edit | retail: a colour edit keeps the decoded bank | none; the node pixel format is not decoded |
| Character sprite bank, raw | 79 | Sprite bank | palette colours | retail, as above | none; 3 banks have no palette layer |
| Area message bank | 200 | Text: records, message-window preview | rewrite a string in place | retail: every string decodes and encodes back | none |
| Area cell grid | 191 | Cell grid over the map size | paint a cell's nibble | retail: a repaint keeps the payload | none; the values have no names yet |
| Area script bank | 200 | Info: directory and sections; Decoded tree | Hex only | retail: decode and encode, 200/200 | the bytecode is not decoded, so records are not editable as values |
| Battle script bank | 42 | Info, Decoded: the area bank's format | Hex only | retail: decode and encode, 42/42 | as the area script bank |
| Area actor definitions | 200 | Tables, Decoded | every sheet cell | retail: every sheet rewrites its bytes | none |
| Area encounters and enemies | 200 | Tables: encounters, enemies, enemy AI | every sheet cell | retail: every sheet rewrites its bytes | none |
| Area map bundle | 200 | Map: cells by tile bank and height | corner heights | retail: decode and encode, 200/200 | none; textures need the tile page, not traced |
| Area auxiliary module window | 9 | Info: module id and body size | Hex only | n/a | area049's window is code, changed in the decompilation; the other eight hold data whose layout is not traced |
| Shared resource table | 44 | Text, Info: id groups | strings in place | retail: strings | none |
| Type-8 cue table | 904 | Tables, Decoded | every sheet cell | retail: every sheet rewrites its bytes | none |
| Magic-effect script bank (`0x800C3800`) | 37 | Info, Decoded: the area bank's format with one section | Hex only | retail: decode and encode, 37/37 | as the area script bank |
| Code image | 548 | Info and Hex; `ETC/GAME.EMI` entry 0 also Tables | Hex; the game tables | retail: sheets | code is changed in the decompilation and rebuilt, not here |
| Reserve (all-zero payload) | 53 | Info and Hex | Hex only | n/a | the payload carries no data; the loader only reserves the space |
| Unclassified | 0 | Info and Hex | Hex only | n/a | no retail entry; types 2 and 9 never occur on the disc |

## Shell

| Feature | State | Gap |
| ------- | ----- | --- |
| Undo and redo | every view, through the project; a drag, a typed field or an import is one step, and undo closes an open field | none |
| Unsaved edits | the status line says `unsaved`; closing the project or the window asks to save or discard them | none |
| Malformed entries | each view reports a decoder error | none; a test draws every view of every format over an empty, a short and a noise payload |

Not EMI entries, so out of scope: XA audio, STR video and CD-DA tracks. The `bof3` command line
handles them.
