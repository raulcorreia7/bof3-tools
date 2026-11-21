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

    if (check(rebof3_abi_version() == REBOF3_ABI_VERSION, "abi version")) return 1;

    psx_emi *emi = NULL;
    if (check(psx_emi_parse(archive, sizeof archive, &emi) == PSX_STATUS_OK, "parse")) return 1;
    uint32_t version = 0;
    if (check(psx_emi_version(emi, &version) == PSX_STATUS_OK && version == 1, "version")) return 1;
    size_t count = 0;
    if (check(psx_emi_entry_count(emi, &count) == PSX_STATUS_OK && count == 1, "entry count")) return 1;
    psx_emi_entry entry;
    if (check(psx_emi_entry_at(emi, 0, &entry) == PSX_STATUS_OK, "entry")) return 1;
    if (check(entry.size == 64 && entry.ram_ptr == 0x80033800u && entry.file_type == 0 && entry.offset == 0x800, "entry fields")) return 1;
    psx_buffer entry_bytes;
    memset(&entry_bytes, 0, sizeof entry_bytes);
    if (check(psx_emi_read_entry(emi, 0, &entry_bytes) == PSX_STATUS_OK, "read entry")) return 1;
    const uint8_t *payload = entry_bytes.data;
    size_t payload_len = entry_bytes.len;
    if (check(payload_len == 64 && payload != NULL && payload[0] == 0x11 && payload[63] == 0x50, "payload bytes")) return 1;

    psx_string json;
    memset(&json, 0, sizeof json);
    if (check(rebof3_emi_manifest_json(emi, false, &json) == PSX_STATUS_OK && json.data != NULL && json.len > 0, "manifest")) return 1;
    psx_string_free(&json);

    rebof3_format format = REBOF3_FORMAT_UNCLASSIFIED;
    if (check(rebof3_format_classify(&entry, payload, payload_len, &format) == PSX_STATUS_OK && format == REBOF3_FORMAT_CLUT_BLOCK, "classify")) return 1;
    rebof3_format_info info;
    memset(&info, 0, sizeof info);
    if (check(rebof3_format_describe(format, &info) == PSX_STATUS_OK && info.name != NULL && strcmp(info.extension, "clut") == 0 && strcmp(info.asset_type, "palette") == 0, "format info")) return 1;
    psx_buffer palette_png;
    memset(&palette_png, 0, sizeof palette_png);
    if (check(rebof3_palette_export_swatch(payload, payload_len, &palette_png) == PSX_STATUS_OK && palette_png.data != NULL && palette_png.len > 8 && palette_png.data[0] == 0x89 && memcmp(palette_png.data + 1, "PNG", 3) == 0, "palette png")) return 1;
    psx_buffer_free(&palette_png);
    psx_buffer palette_json;
    memset(&palette_json, 0, sizeof palette_json);
    if (check(rebof3_palette_export_json(payload, payload_len, &palette_json) == PSX_STATUS_OK && palette_json.data != NULL && palette_json.len > 0 && palette_json.data[0] == '{', "palette json")) return 1;
    psx_buffer_free(&palette_json);
    static const uint8_t vag[32] = {0};
    psx_buffer wav;
    memset(&wav, 0, sizeof wav);
    if (check(rebof3_vag_export_wav(vag, sizeof vag, &wav) == PSX_STATUS_OK && wav.data != NULL && wav.len > 44 && memcmp(wav.data, "RIFF", 4) == 0, "vag wav")) return 1;
    psx_buffer_free(&wav);

    size_t codecs = 0;
    if (check(rebof3_assets_codec_count(&codecs) == PSX_STATUS_OK && codecs > 0, "codec count")) return 1;
    rebof3_assets_codec codec;
    memset(&codec, 0, sizeof codec);
    if (check(rebof3_assets_codec_info("palette", 7, REBOF3_DIRECTION_EXPORT, &codec) == PSX_STATUS_OK, "codec info")) return 1;
    if (check(codec.asset_type != NULL && strcmp(codec.asset_type, "palette") == 0 && codec.extension != NULL && strcmp(codec.extension, "json") == 0 && codec.files == 1 && !codec.named && !codec.load_arg && codec.index == REBOF3_INDEX_NONE, "codec fields")) return 1;
    rebof3_assets_job job;
    memset(&job, 0, sizeof job);
    job.input = payload;
    job.input_len = payload_len;
    psx_buffer converted;
    memset(&converted, 0, sizeof converted);
    if (check(rebof3_assets_convert("palette", 7, REBOF3_DIRECTION_EXPORT, &job, &converted) == PSX_STATUS_OK && converted.data != NULL && converted.len > 0 && converted.data[0] == '{', "assets convert")) return 1;
    psx_buffer_free(&converted);
    if (check(rebof3_assets_convert("no-such-type", strlen("no-such-type"), REBOF3_DIRECTION_EXPORT, &job, &converted) == PSX_STATUS_UNSUPPORTED, "unknown codec")) return 1;
    if (check(rebof3_assets_extract(NULL, NULL, NULL) == PSX_STATUS_INVALID_ARGUMENT, "extract null options")) return 1;

    psx_buffer rebuilt;
    memset(&rebuilt, 0, sizeof rebuilt);
    if (check(rebof3_emi_rebuild(archive, sizeof archive, NULL, 0, &rebuilt) == PSX_STATUS_OK && rebuilt.data != NULL, "rebuild")) return 1;
    psx_buffer_free(&rebuilt);

    psx_emi *bad = NULL;
    if (check(psx_emi_parse(NULL, 1, &bad) == PSX_STATUS_INVALID_ARGUMENT, "null parse")) return 1;
    if (check(psx_last_error_message() != NULL, "null parse message")) return 1;
    if (check(psx_emi_entry_at(emi, 9, &entry) == PSX_STATUS_INVALID_INDEX, "bad index")) return 1;
    if (check(psx_emi_parse(archive, 16, &bad) == PSX_STATUS_FORMAT, "bad archive")) return 1;
    if (check(psx_last_error_message() != NULL, "bad archive message")) return 1;

    psx_buffer_free(&entry_bytes);
    psx_emi_free(emi);
    psx_emi_free(NULL);
    return 0;
}
