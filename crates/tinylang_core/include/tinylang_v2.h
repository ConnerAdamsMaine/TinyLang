#ifndef TINYLANG_V2_H
#define TINYLANG_V2_H

#include <stdint.h>

#if defined(_WIN32) && defined(TINYLANG_V2_BUILD)
#define TINYLANG_V2_API __declspec(dllexport)
#elif defined(_WIN32)
#define TINYLANG_V2_API __declspec(dllimport)
#elif defined(__GNUC__) || defined(__clang__)
#define TINYLANG_V2_API __attribute__((visibility("default")))
#else
#define TINYLANG_V2_API
#endif

#ifdef __cplusplus
extern "C" {
#endif

#define TINYLANG_V2_ABI_VERSION UINT32_C(0x00020000)
#define TINYLANG_V2_HEADER_BYTES UINT32_C(32)
#define TINYLANG_V2_VALUE_HEADER_BYTES UINT32_C(16)
#define TINYLANG_V2_MAX_FRAME_BYTES UINT64_C(67108864)

typedef uint32_t TinyStatus;
typedef uint64_t TinyRuntimeHandle;
typedef uint64_t TinyHandle;

#define TINY_STATUS_OK UINT32_C(0)
#define TINY_STATUS_INVALID_ARGUMENT UINT32_C(1)
#define TINY_STATUS_INVALID_FRAME UINT32_C(2)
#define TINY_STATUS_UNSUPPORTED_VERSION UINT32_C(3)
#define TINY_STATUS_UNKNOWN_OPCODE UINT32_C(4)
#define TINY_STATUS_BUFFER_TOO_SMALL UINT32_C(5)
#define TINY_STATUS_INVALID_HANDLE UINT32_C(6)
#define TINY_STATUS_STALE_HANDLE UINT32_C(7)
#define TINY_STATUS_OUT_OF_MEMORY UINT32_C(8)
#define TINY_STATUS_VM_TRAP UINT32_C(9)
#define TINY_STATUS_INTERNAL UINT32_C(10)
#define TINY_STATUS_PANIC UINT32_C(11)
#define TINY_STATUS_BUSY UINT32_C(12)
#define TINY_STATUS_INVALID_ARTIFACT UINT32_C(13)

#define TINY_FLAG_RESPONSE UINT32_C(1)

#define TINY_OPCODE_NOP UINT16_C(0x0000)
#define TINY_OPCODE_ABI_INFO UINT16_C(0x0001)
#define TINY_OPCODE_ECHO UINT16_C(0x0002)
#define TINY_OPCODE_PROGRAM_LOAD UINT16_C(0x1000)
#define TINY_OPCODE_VM_CREATE UINT16_C(0x1001)
#define TINY_OPCODE_VM_INVOKE UINT16_C(0x1002)
#define TINY_OPCODE_HANDLE_RELEASE UINT16_C(0x1003)

#define TINY_VM_MAX_HEAP_BYTES UINT64_C(4194304)
#define TINY_VM_MAX_HEAP_OBJECTS UINT32_C(1000000)
#define TINY_VM_MAX_CALL_DEPTH UINT32_C(16)
#define TINY_VM_FLAG_ALLOW_JIT UINT32_C(1)

#define TINY_VALUE_NULL UINT16_C(0)
#define TINY_VALUE_BOOL UINT16_C(1)
#define TINY_VALUE_U8 UINT16_C(2)
#define TINY_VALUE_U16 UINT16_C(3)
#define TINY_VALUE_U32 UINT16_C(4)
#define TINY_VALUE_U64 UINT16_C(5)
#define TINY_VALUE_I8 UINT16_C(6)
#define TINY_VALUE_I16 UINT16_C(7)
#define TINY_VALUE_I32 UINT16_C(8)
#define TINY_VALUE_I64 UINT16_C(9)
#define TINY_VALUE_F32 UINT16_C(10)
#define TINY_VALUE_F64 UINT16_C(11)
#define TINY_VALUE_BYTES UINT16_C(12)
#define TINY_VALUE_UTF8 UINT16_C(13)
#define TINY_VALUE_HANDLE UINT16_C(14)
#define TINY_VALUE_ENUM UINT16_C(15)
#define TINY_VALUE_SLICE UINT16_C(16)
#define TINY_VALUE_SEQUENCE UINT16_C(17)

/* Borrowed, immutable host memory. Null is allowed only when length is zero. */
typedef struct TinyBuffer {
    const uint8_t *data;
    uint64_t length;
} TinyBuffer;

/* Borrowed, writable host memory. Null is allowed only when capacity is zero. */
typedef struct TinyMutableBuffer {
    uint8_t *data;
    uint64_t capacity;
} TinyMutableBuffer;

/* Runtime-owned memory. Do not modify data, capacity, or owner_token. */
typedef struct TinyOwnedBuffer {
    uint8_t *data;
    uint64_t length;
    uint64_t capacity;
    uint64_t owner_token;
} TinyOwnedBuffer;

TINYLANG_V2_API uint32_t tinylang_v2_abi_version(void);

TINYLANG_V2_API TinyStatus
tinylang_v2_runtime_create(TinyRuntimeHandle *out_runtime);

/* Returns TINY_STATUS_BUSY while runtime-owned buffers remain outstanding. */
TINYLANG_V2_API TinyStatus
tinylang_v2_runtime_destroy(TinyRuntimeHandle runtime);

/* Optional runtime-owned transfer storage; raw call buffers remain host-owned. */
TINYLANG_V2_API TinyStatus tinylang_v2_buffer_acquire(
    TinyRuntimeHandle runtime,
    uint64_t minimum_capacity,
    TinyOwnedBuffer *out_buffer);

/* Clears *buffer on success. Every copied old descriptor is then invalid. */
TINYLANG_V2_API TinyStatus tinylang_v2_buffer_release(
    TinyRuntimeHandle runtime,
    TinyOwnedBuffer *buffer);

/*
 * Validates and dispatches one request frame.
 *
 * request, response, and out_written must not overlap. No pointer is retained.
 * On TINY_STATUS_BUFFER_TOO_SMALL, *out_written is the required capacity and
 * response memory is unchanged. A TINY_STATUS_OK return means a response frame
 * was written; application/VM status is stored in that frame's status field.
 */
TINYLANG_V2_API TinyStatus tinylang_v2_call(
    TinyRuntimeHandle runtime,
    TinyBuffer request,
    TinyMutableBuffer response,
    uint64_t *out_written);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* TINYLANG_V2_H */
