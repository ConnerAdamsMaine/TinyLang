#ifndef TYNRA_V2_H
#define TYNRA_V2_H

#include <stdint.h>

#if defined(_WIN32) && defined(TYNRA_V2_BUILD)
#define TYNRA_V2_API __declspec(dllexport)
#elif defined(_WIN32)
#define TYNRA_V2_API __declspec(dllimport)
#elif defined(__GNUC__) || defined(__clang__)
#define TYNRA_V2_API __attribute__((visibility("default")))
#else
#define TYNRA_V2_API
#endif

#ifdef __cplusplus
extern "C" {
#endif

#define TYNRA_V2_ABI_VERSION UINT32_C(0x00020000)
#define TYNRA_V2_HEADER_BYTES UINT32_C(32)
#define TYNRA_V2_VALUE_HEADER_BYTES UINT32_C(16)
#define TYNRA_V2_MAX_FRAME_BYTES UINT64_C(67108864)

typedef uint32_t TynraStatus;
typedef uint64_t TynraRuntimeHandle;
typedef uint64_t TynraHandle;

#define TYNRA_STATUS_OK UINT32_C(0)
#define TYNRA_STATUS_INVALID_ARGUMENT UINT32_C(1)
#define TYNRA_STATUS_INVALID_FRAME UINT32_C(2)
#define TYNRA_STATUS_UNSUPPORTED_VERSION UINT32_C(3)
#define TYNRA_STATUS_UNKNOWN_OPCODE UINT32_C(4)
#define TYNRA_STATUS_BUFFER_TOO_SMALL UINT32_C(5)
#define TYNRA_STATUS_INVALID_HANDLE UINT32_C(6)
#define TYNRA_STATUS_STALE_HANDLE UINT32_C(7)
#define TYNRA_STATUS_OUT_OF_MEMORY UINT32_C(8)
#define TYNRA_STATUS_VM_TRAP UINT32_C(9)
#define TYNRA_STATUS_INTERNAL UINT32_C(10)
#define TYNRA_STATUS_PANIC UINT32_C(11)
#define TYNRA_STATUS_BUSY UINT32_C(12)
#define TYNRA_STATUS_INVALID_ARTIFACT UINT32_C(13)

#define TYNRA_FLAG_RESPONSE UINT32_C(1)

#define TYNRA_OPCODE_NOP UINT16_C(0x0000)
#define TYNRA_OPCODE_ABI_INFO UINT16_C(0x0001)
#define TYNRA_OPCODE_ECHO UINT16_C(0x0002)
#define TYNRA_OPCODE_PROGRAM_LOAD UINT16_C(0x1000)
#define TYNRA_OPCODE_VM_CREATE UINT16_C(0x1001)
#define TYNRA_OPCODE_VM_INVOKE UINT16_C(0x1002)
#define TYNRA_OPCODE_HANDLE_RELEASE UINT16_C(0x1003)

#define TYNRA_VM_MAX_HEAP_BYTES UINT64_C(4194304)
#define TYNRA_VM_MAX_HEAP_OBJECTS UINT32_C(1000000)
#define TYNRA_VM_MAX_CALL_DEPTH UINT32_C(16)
#define TYNRA_VM_FLAG_ALLOW_JIT UINT32_C(1)

#define TYNRA_VALUE_NULL UINT16_C(0)
#define TYNRA_VALUE_BOOL UINT16_C(1)
#define TYNRA_VALUE_U8 UINT16_C(2)
#define TYNRA_VALUE_U16 UINT16_C(3)
#define TYNRA_VALUE_U32 UINT16_C(4)
#define TYNRA_VALUE_U64 UINT16_C(5)
#define TYNRA_VALUE_I8 UINT16_C(6)
#define TYNRA_VALUE_I16 UINT16_C(7)
#define TYNRA_VALUE_I32 UINT16_C(8)
#define TYNRA_VALUE_I64 UINT16_C(9)
#define TYNRA_VALUE_F32 UINT16_C(10)
#define TYNRA_VALUE_F64 UINT16_C(11)
#define TYNRA_VALUE_BYTES UINT16_C(12)
#define TYNRA_VALUE_UTF8 UINT16_C(13)
#define TYNRA_VALUE_HANDLE UINT16_C(14)
#define TYNRA_VALUE_ENUM UINT16_C(15)
#define TYNRA_VALUE_SLICE UINT16_C(16)
#define TYNRA_VALUE_SEQUENCE UINT16_C(17)

/* Borrowed, immutable host memory. Null is allowed only when length is zero. */
typedef struct TynraBuffer {
    const uint8_t *data;
    uint64_t length;
} TynraBuffer;

/* Borrowed, writable host memory. Null is allowed only when capacity is zero. */
typedef struct TynraMutableBuffer {
    uint8_t *data;
    uint64_t capacity;
} TynraMutableBuffer;

/* Runtime-owned memory. Do not modify data, capacity, or owner_token. */
typedef struct TynraOwnedBuffer {
    uint8_t *data;
    uint64_t length;
    uint64_t capacity;
    uint64_t owner_token;
} TynraOwnedBuffer;

TYNRA_V2_API uint32_t tynra_v2_abi_version(void);

TYNRA_V2_API TynraStatus
tynra_v2_runtime_create(TynraRuntimeHandle *out_runtime);

/* Returns TYNRA_STATUS_BUSY while runtime-owned buffers remain outstanding. */
TYNRA_V2_API TynraStatus
tynra_v2_runtime_destroy(TynraRuntimeHandle runtime);

/* EXPERIMENTAL (not part of any stable-client surface in 2.x): optional
 * runtime-owned transfer storage; raw call buffers remain host-owned.
 * The ABA/lifetime argument for the owner token is documented in
 * docs/abi/v2-binary-spec.md. Stable clients must not rely on these calls. */
TYNRA_V2_API TynraStatus tynra_v2_buffer_acquire(
    TynraRuntimeHandle runtime,
    uint64_t minimum_capacity,
    TynraOwnedBuffer *out_buffer);

/* EXPERIMENTAL (not part of any stable-client surface in 2.x).
 * Clears *buffer on success. Every copied old descriptor is then invalid. */
TYNRA_V2_API TynraStatus tynra_v2_buffer_release(
    TynraRuntimeHandle runtime,
    TynraOwnedBuffer *buffer);

/*
 * Validates and dispatches one request frame.
 *
 * request, response, and out_written must not overlap. No pointer is retained.
 * On TYNRA_STATUS_BUFFER_TOO_SMALL, *out_written is the required capacity and
 * response memory is unchanged. A TYNRA_STATUS_OK return means a response frame
 * was written; application/VM status is stored in that frame's status field.
 */
TYNRA_V2_API TynraStatus tynra_v2_call(
    TynraRuntimeHandle runtime,
    TynraBuffer request,
    TynraMutableBuffer response,
    uint64_t *out_written);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* TYNRA_V2_H */
