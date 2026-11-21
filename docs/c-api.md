# C interface

`rebof3-capi` is the C ABI over the library crates. The library crates stay pure Rust; the crate
holds every `extern "C"` function, the status codes and the ownership rules. It exports the EMI
container, the entry formats, and the asset codecs and extractor.

## Build

```sh
cargo build --release -p rebof3-capi --locked
```

The build writes `target/release/librebof3_capi.so` and `librebof3_capi.a` on Linux and macOS, and
`rebof3_capi.dll` and `rebof3_capi.lib` on Windows. Link the shared library when the target language
loads it at run time; link the static library with the native libraries Cargo names:

```sh
cargo rustc -p rebof3-capi --release --locked -- --print native-static-libs
```

The header is [capi/include/rebof3_capi.h](../capi/include/rebof3_capi.h). It includes
[capi/include/psx_capi.h](../capi/include/psx_capi.h), the `psx-capi` layer shared with
`libbof3player_capi`: `psx_status`, `psx_last_error_message`, `psx_buffer`, `psx_string` and the EMI
handle. `make check` fails when either header is stale. Regenerate them with:

```sh
UPDATE_HEADER=1 cargo test -p rebof3-capi --test header
```

## Formats

`rebof3_format_classify` names an entry's format from its table record and payload.
`rebof3_format_describe` returns the format's static name, extension, state and asset kind; those
strings are NUL-terminated and are not freed. The exporters are `rebof3_texture_export_png`,
`rebof3_texture_export_tim`, `rebof3_texture_export_row_png`, `rebof3_palette_export_json`,
`rebof3_palette_export_swatch`, `rebof3_vag_export_wav`, `rebof3_vab_export_wav`,
`rebof3_vab_export_soundfont` and `rebof3_sprite_sheet_export`. Each returns an owned buffer.

## EMI archives

Open with `psx_emi_open_path` or `psx_emi_parse`; `psx_emi_read_entry` copies one entry. Extraction
needs a handle from `psx_emi_open_path`. `rebof3_emi_rebuild` and `rebof3_emi_pack_payloads` return
a new container in memory.

## Assets

`rebof3_assets_codec_count` and `rebof3_assets_codec_info` describe the conversion table: a kind's
files, whether it takes a name, an index or a load argument, and its default extension; the kind and
extension strings are static and NUL-terminated. `rebof3_assets_convert` runs one export or import
from memory. `rebof3_assets_extract` runs the selected kinds over a disc image and returns the
`assets.jsonl` text and summary; with `write` set it writes them into the output directory too. A
NULL `kinds` selects every kind, with the `text` kind included only when `text_disc` is set.

## Use

Every fallible function returns a `psx_status`. On a failure, `psx_last_error_message` returns the
calling thread's message until the next call on that thread; `PSX_STATUS_PANIC` means the library
caught a Rust panic at the boundary. `rebof3_abi_version()` equals `REBOF3_ABI_VERSION` in the header;
it is 2 and changes only when an existing signature or layout changes. Flags are `bool` and
out-parameters are named `out_<noun>`.

Ownership:

- Every owned result has one `_free` that takes a pointer to it, accepts NULL and zeroes it:
  `psx_buffer_free`, `psx_string_free`, `psx_emi_free` and `rebof3_sprite_sheet_free`.
- Borrowed pointers say so in the header: input records and their pointers are read during the call
  only, the last-error message lasts until the next call on the thread, and the strings of
  `rebof3_format_info` and `rebof3_assets_codec` live for the process and are not freed.
- Strings go in as a UTF-8 pointer and length, without a NUL terminator. Paths are UTF-8 too.

A NULL pointer is valid only with a zero length.

## Example

```c
#include <stdio.h>
#include <string.h>

#include "rebof3_capi.h"

int main(int argc, char **argv) {
    if (argc != 2) return 2;
    psx_emi *emi = NULL;
    if (psx_emi_open_path(argv[1], strlen(argv[1]), &emi) != PSX_STATUS_OK) {
        fprintf(stderr, "%s\n", psx_last_error_message());
        return 1;
    }
    size_t count = 0;
    psx_emi_entry_count(emi, &count);
    for (size_t index = 0; index < count; index++) {
        psx_emi_entry entry;
        if (psx_emi_entry_at(emi, index, &entry) != PSX_STATUS_OK) break;
        printf("%zu: type %u, load %08x, %u bytes\n", index, entry.file_type, entry.ram_ptr, entry.size);
    }
    psx_emi_free(emi);
    return 0;
}
```

```sh
cc example.c -I capi/include -L target/release -lrebof3_capi -o example
LD_LIBRARY_PATH=target/release ./example build/disc/BIN/BGM/BGM000.EMI
```
