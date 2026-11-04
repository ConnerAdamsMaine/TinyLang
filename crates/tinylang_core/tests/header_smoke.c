#include "tinylang_v2.h"

#include <stddef.h>
#include <stdint.h>

_Static_assert(sizeof(TinyStatus) == 4, "TinyStatus must be 32 bits");
_Static_assert(sizeof(TinyRuntimeHandle) == 8, "runtime handles must be 64 bits");
_Static_assert(sizeof(TinyHandle) == 8, "VM handles must be 64 bits");
_Static_assert(sizeof(((TinyBuffer *)0)->length) == 8, "buffer length must be 64 bits");
_Static_assert(sizeof(((TinyMutableBuffer *)0)->capacity) == 8,
               "buffer capacity must be 64 bits");
_Static_assert(offsetof(TinyOwnedBuffer, owner_token) > offsetof(TinyOwnedBuffer, capacity),
               "owned buffer field order changed");

int tinylang_v2_header_smoke(void) {
    TinyBuffer input = {NULL, 0};
    TinyMutableBuffer output = {NULL, 0};
    TinyOwnedBuffer owned = {NULL, 0, 0, 0};
    TinyRuntimeHandle runtime = 0;
    uint64_t written = 0;

    (void)input;
    (void)output;
    (void)owned;
    (void)runtime;
    (void)written;
    return TINYLANG_V2_ABI_VERSION == UINT32_C(0x00020000) ? 0 : 1;
}

