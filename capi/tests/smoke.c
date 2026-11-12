/* Exercises the core and EMI functions on synthetic bytes. */
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "rebof3_capi.h"

static int check(int condition, const char *what) {
    if (!condition) {
        fprintf(stderr, "smoke: %s failed\n", what);
        return 1;
    }
    return 0;
}

int rebof3_smoke_run(void) {
    static uint8_t archive[0x800 + 64];
    archive[0] = 1; /* one entry */
    archive[4] = 1; /* version */
    memcpy(archive + 8, "MATH_TBL", 8);
    archive[16] = 64; /* entry size */
    archive[20] = 0x00; /* load argument 0x80033800 */
    archive[21] = 0x38;
    archive[22] = 0x03;
    archive[23] = 0x80;
    archive[28] = 0; /* file type */
    archive[29] = 0;
    archive[30] = 0x2e; /* pad */
    archive[31] = 0x2e;
    for (int i = 0; i < 64; i++) {
        archive[0x800 + i] = (uint8_t)(0x11 + i);
    }

    if (check(rebof3_abi_version() == 1, "abi version")) return 1;

    rebof3_emi *emi = NULL;
    if (check(rebof3_emi_parse(archive, sizeof archive, &emi) == REBOF3_STATUS_OK, "parse")) return 1;
    uint32_t version = 0;
    if (check(rebof3_emi_version(emi, &version) == REBOF3_STATUS_OK && version == 1, "version")) return 1;
    size_t count = 0;
    if (check(rebof3_emi_entry_count(emi, &count) == REBOF3_STATUS_OK && count == 1, "entry count")) return 1;
    rebof3_emi_entry entry;
    if (check(rebof3_emi_entry_at(emi, 0, &entry) == REBOF3_STATUS_OK, "entry")) return 1;
    if (check(entry.size == 64 && entry.ram_ptr == 0x80033800u && entry.file_type == 0 && entry.offset == 0x800, "entry fields")) return 1;
    const uint8_t *payload = NULL;
    size_t payload_len = 0;
    if (check(rebof3_emi_payload_at(emi, archive, sizeof archive, 0, &payload, &payload_len) == REBOF3_STATUS_OK, "payload")) return 1;
    if (check(payload_len == 64 && payload != NULL && payload[0] == 0x11 && payload[63] == 0x50, "payload bytes")) return 1;

    rebof3_string json;
    memset(&json, 0, sizeof json);
    if (check(rebof3_emi_manifest_json(emi, 0, &json) == REBOF3_STATUS_OK && json.data != NULL && json.len > 0, "manifest")) return 1;
    rebof3_string_free(&json);

    rebof3_buffer rebuilt;
    memset(&rebuilt, 0, sizeof rebuilt);
    if (check(rebof3_emi_rebuild(archive, sizeof archive, NULL, 0, &rebuilt) == REBOF3_STATUS_OK && rebuilt.data != NULL, "rebuild")) return 1;
    rebof3_buffer_free(&rebuilt);

    rebof3_emi *bad = NULL;
    if (check(rebof3_emi_parse(NULL, 1, &bad) == REBOF3_STATUS_INVALID_ARGUMENT, "null parse")) return 1;
    if (check(rebof3_last_error_message() != NULL, "null parse message")) return 1;
    if (check(rebof3_emi_entry_at(emi, 9, &entry) == REBOF3_STATUS_INVALID_INDEX, "bad index")) return 1;
    if (check(rebof3_emi_parse(archive, 16, &bad) == REBOF3_STATUS_FORMAT, "bad archive")) return 1;
    if (check(rebof3_last_error_message() != NULL, "bad archive message")) return 1;

    rebof3_emi_free(emi);
    rebof3_emi_free(NULL);
    return 0;
}
