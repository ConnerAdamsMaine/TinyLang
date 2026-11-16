#include "tynra_v2.h"

#include <stddef.h>
#include <stdint.h>

_Static_assert(sizeof(TynraStatus) == 4, "TynraStatus must be 32 bits");
_Static_assert(sizeof(TynraRuntimeHandle) == 8, "runtime handles must be 64 bits");
_Static_assert(sizeof(TynraHandle) == 8, "VM handles must be 64 bits");
_Static_assert(sizeof(((TynraBuffer *)0)->length) == 8, "buffer length must be 64 bits");
_Static_assert(sizeof(((TynraMutableBuffer *)0)->capacity) == 8,
               "buffer capacity must be 64 bits");
_Static_assert(offsetof(TynraOwnedBuffer, owner_token) > offsetof(TynraOwnedBuffer, capacity),
               "owned buffer field order changed");

int tynra_v2_header_smoke(void) {
    TynraBuffer input = {NULL, 0};
    TynraMutableBuffer output = {NULL, 0};
    TynraOwnedBuffer owned = {NULL, 0, 0, 0};
    TynraRuntimeHandle runtime = 0;
    uint64_t written = 0;

    (void)input;
    (void)output;
    (void)owned;
    (void)runtime;
    (void)written;
    return TYNRA_V2_ABI_VERSION == UINT32_C(0x00020000) ? 0 : 1;
}

