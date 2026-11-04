#include "tinylang_v2.h"

#include <stdint.h>
#include <stdio.h>
#include <string.h>

static void put_u16_le(uint8_t *bytes, uint16_t value) {
    bytes[0] = (uint8_t)value;
    bytes[1] = (uint8_t)(value >> 8);
}

static void put_u32_le(uint8_t *bytes, uint32_t value) {
    bytes[0] = (uint8_t)value;
    bytes[1] = (uint8_t)(value >> 8);
    bytes[2] = (uint8_t)(value >> 16);
    bytes[3] = (uint8_t)(value >> 24);
}

static void put_u64_le(uint8_t *bytes, uint64_t value) {
    unsigned int index;
    for (index = 0; index < 8; ++index) {
        bytes[index] = (uint8_t)(value >> (index * 8));
    }
}

static uint32_t get_u32_le(const uint8_t *bytes) {
    return (uint32_t)bytes[0] | ((uint32_t)bytes[1] << 8) |
           ((uint32_t)bytes[2] << 16) | ((uint32_t)bytes[3] << 24);
}

static uint64_t get_u64_le(const uint8_t *bytes) {
    uint64_t value = 0;
    unsigned int index;
    for (index = 0; index < 8; ++index) {
        value |= (uint64_t)bytes[index] << (index * 8);
    }
    return value;
}

static int fail(const char *message) {
    (void)fprintf(stderr, "TinyLang v2 C smoke failure: %s\n", message);
    return 1;
}

int main(void) {
    static const uint64_t correlation_id = UINT64_C(0x0102030405060708);
    uint8_t request[TINYLANG_V2_HEADER_BYTES] = {0};
    uint8_t response[TINYLANG_V2_HEADER_BYTES];
    TinyRuntimeHandle runtime = 0;
    TinyBuffer input;
    TinyMutableBuffer output;
    uint64_t written = 0;
    TinyStatus status;

    if (tinylang_v2_abi_version() != TINYLANG_V2_ABI_VERSION) {
        return fail("ABI version mismatch");
    }
    status = tinylang_v2_runtime_create(&runtime);
    if (status != TINY_STATUS_OK || runtime == 0) {
        return fail("runtime creation failed");
    }

    (void)memcpy(request, "TLV2", 4);
    put_u16_le(request + 4, 2);
    put_u16_le(request + 6, 0);
    put_u16_le(request + 8, TINYLANG_V2_HEADER_BYTES);
    put_u16_le(request + 10, TINY_OPCODE_NOP);
    put_u64_le(request + 24, correlation_id);
    (void)memset(response, 0xa5, sizeof(response));

    input.data = request;
    input.length = sizeof(request);
    output.data = response;
    output.capacity = sizeof(response);
    status = tinylang_v2_call(runtime, input, output, &written);
    if (status != TINY_STATUS_OK || written != sizeof(response)) {
        return fail("NOP call failed");
    }
    if (memcmp(response, "TLV2", 4) != 0 ||
        get_u32_le(response + 12) != TINY_FLAG_RESPONSE ||
        get_u32_le(response + 16) != TINY_STATUS_OK ||
        get_u32_le(response + 20) != 0 ||
        get_u64_le(response + 24) != correlation_id) {
        return fail("NOP response was not canonical");
    }
    if (tinylang_v2_runtime_destroy(runtime) != TINY_STATUS_OK) {
        return fail("runtime destruction failed");
    }
    return 0;
}
