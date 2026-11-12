# C interface

`rebof3-capi` is the C ABI over the library crates. The library crates stay pure Rust; the crate
holds every `extern "C"` function, the status codes and the ownership rules. It starts with the EMI
container; the format and asset functions follow.

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

The header is [capi/include/rebof3_capi.h](../capi/include/rebof3_capi.h). It is generated from the
crate; `make check` fails when the committed header is stale. Regenerate it with:

```sh
UPDATE_HEADER=1 cargo test -p rebof3-capi --test header
```

## Use

Every fallible function returns a `rebof3_status`. On a failure, `rebof3_last_error_message`
returns the calling thread's message until the next call on that thread; `REBOF3_STATUS_PANIC` means
the library caught a Rust panic at the boundary. `rebof3_abi_version()` is 1 and changes only when an
existing signature or layout changes.

Ownership:

- A `rebof3_buffer` or `rebof3_string` returned by a function is owned by the caller and released
  with `rebof3_buffer_free` or `rebof3_string_free`.
- A `rebof3_emi` handle is released with `rebof3_emi_free`.
- `rebof3_emi_payload_at` borrows the container bytes the caller parsed; the pointer is valid while
  those bytes are.
- Strings the header describes as static, and the fields of `rebof3_format_info`, are not freed.
- Strings go in as a UTF-8 pointer and length, without a NUL terminator. Paths are UTF-8 too.

A NULL pointer is valid only with a zero length. The error message is thread-local; a handle may be
read from several threads, but `_free` needs external synchronization.

## Example

```c
#include <stdio.h>
#include <string.h>

#include "rebof3_capi.h"

int main(int argc, char **argv) {
    if (argc != 2) return 2;
    rebof3_emi *emi = NULL;
    if (rebof3_emi_open_path(argv[1], strlen(argv[1]), &emi) != REBOF3_STATUS_OK) {
        fprintf(stderr, "%s\n", rebof3_last_error_message());
        return 1;
    }
    size_t count = 0;
    rebof3_emi_entry_count(emi, &count);
    for (size_t index = 0; index < count; index++) {
        rebof3_emi_entry entry;
        if (rebof3_emi_entry_at(emi, index, &entry) != REBOF3_STATUS_OK) break;
        printf("%zu: type %u, load %08x, %u bytes\n", index, entry.file_type, entry.ram_ptr, entry.size);
    }
    rebof3_emi_free(emi);
    return 0;
}
```

```sh
cc example.c -I capi/include -L target/release -lrebof3_capi -o example
LD_LIBRARY_PATH=target/release ./example build/disc/BIN/BGM/BGM000.EMI
```
