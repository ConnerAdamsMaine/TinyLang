"""Minimal dependency-free ctypes binding for the TinyLang v2 raw ABI."""

from __future__ import annotations

import ctypes
import struct
from enum import IntEnum
from pathlib import Path
from typing import NamedTuple

ABI_VERSION = 0x0002_0000
HEADER_BYTES = 32
MAX_FRAME_BYTES = 64 * 1024 * 1024
FLAG_RESPONSE = 1

STATUS_OK = 0
STATUS_BUFFER_TOO_SMALL = 5
STATUS_INVALID_HANDLE = 6

OPCODE_NOP = 0x0000
OPCODE_ABI_INFO = 0x0001
OPCODE_ECHO = 0x0002
OPCODE_PROGRAM_LOAD = 0x1000
OPCODE_VM_CREATE = 0x1001
OPCODE_HANDLE_RELEASE = 0x1003

VALUE_U32 = 4
VALUE_U64 = 5
VALUE_BYTES = 12
VALUE_HANDLE = 14

_U8_POINTER = ctypes.POINTER(ctypes.c_uint8)


class ArtifactFormat(IntEnum):
    """Explicit PROGRAM_LOAD decoder; compiler versions remain experimental."""

    TLBC = 1
    COMPILER_BINARY = 2


class TinyBuffer(ctypes.Structure):
    _fields_ = [("data", _U8_POINTER), ("length", ctypes.c_uint64)]


class TinyMutableBuffer(ctypes.Structure):
    _fields_ = [("data", _U8_POINTER), ("capacity", ctypes.c_uint64)]


class ResponseHeader(NamedTuple):
    opcode: int
    flags: int
    status: int
    payload_length: int
    correlation_id: int


def make_request(opcode: int, payload: bytes = b"", correlation_id: int = 0) -> bytes:
    """Encode one canonical v2 request envelope around pre-encoded values."""
    if not 0 <= opcode <= 0xFFFF:
        raise ValueError("opcode does not fit u16")
    if not 0 <= correlation_id <= 0xFFFF_FFFF_FFFF_FFFF:
        raise ValueError("correlation_id does not fit u64")
    payload = bytes(payload)
    if len(payload) > 0xFFFF_FFFF or HEADER_BYTES + len(payload) > MAX_FRAME_BYTES:
        raise ValueError("payload exceeds the v2 frame limit")
    return struct.pack(
        "<4sHHHHIIIQ",
        b"TLV2",
        2,
        0,
        HEADER_BYTES,
        opcode,
        0,
        0,
        len(payload),
        correlation_id,
    ) + payload


def value_record(tag: int, payload: bytes = b"", auxiliary: int = 0) -> bytes:
    """Encode one canonical, zero-padded value record."""
    if not 0 <= tag <= 0xFFFF:
        raise ValueError("value tag does not fit u16")
    if not 0 <= auxiliary <= 0xFFFF_FFFF_FFFF_FFFF:
        raise ValueError("value auxiliary does not fit u64")
    payload = bytes(payload)
    if len(payload) > 0xFFFF_FFFF:
        raise ValueError("value payload does not fit u32")
    unpadded = 16 + len(payload)
    padding = (-unpadded) & 7
    return struct.pack("<HHIQ", tag, 0, len(payload), auxiliary) + payload + bytes(padding)


def u32_value(value: int) -> bytes:
    if not 0 <= value <= 0xFFFF_FFFF:
        raise ValueError("value does not fit u32")
    return value_record(VALUE_U32, struct.pack("<I", value))


def u64_value(value: int) -> bytes:
    if not 0 <= value <= 0xFFFF_FFFF_FFFF_FFFF:
        raise ValueError("value does not fit u64")
    return value_record(VALUE_U64, struct.pack("<Q", value))


def handle_value(handle: int) -> bytes:
    return value_record(VALUE_HANDLE, auxiliary=handle)


def bytes_value(value: bytes) -> bytes:
    return value_record(VALUE_BYTES, value)


def program_load_payload(
    artifact: bytes | bytearray | memoryview,
    *,
    artifact_format: ArtifactFormat | int = ArtifactFormat.TLBC,
) -> bytes:
    """Encode TLBC by default, or explicitly select compiler binary format2."""
    if (
        isinstance(artifact_format, bool)
        or not isinstance(artifact_format, int)
        or artifact_format not in (1, 2)
    ):
        raise ValueError("artifact_format must be 1 (TLBC) or 2 (compiler binary)")
    selected = ArtifactFormat(artifact_format)
    artifact = bytes(artifact)
    limit = 16 * 1024 * 1024 if selected is ArtifactFormat.TLBC else 8 * 1024 * 1024
    if not 1 <= len(artifact) <= limit:
        raise ValueError(f"artifact length must be between 1 and {limit} bytes")
    return u32_value(selected) + bytes_value(artifact)


def make_program_load_request(
    artifact: bytes | bytearray | memoryview,
    correlation_id: int = 0,
    *,
    artifact_format: ArtifactFormat | int = ArtifactFormat.TLBC,
) -> bytes:
    payload = program_load_payload(artifact, artifact_format=artifact_format)
    return make_request(OPCODE_PROGRAM_LOAD, payload, correlation_id)


def make_handle_release_request(handle: int, correlation_id: int = 0) -> bytes:
    return make_request(OPCODE_HANDLE_RELEASE, handle_value(handle), correlation_id)


def make_vm_create_request(
    program: int,
    max_heap_bytes: int,
    max_heap_objects: int,
    max_call_depth: int,
    flags: int = 0,
    correlation_id: int = 0,
) -> bytes:
    payload = b"".join(
        (
            handle_value(program),
            u64_value(max_heap_bytes),
            u32_value(max_heap_objects),
            u32_value(max_call_depth),
            u32_value(flags),
        )
    )
    return make_request(OPCODE_VM_CREATE, payload, correlation_id)


def parse_response(frame: bytes) -> ResponseHeader:
    """Validate a complete response envelope and return its scalar fields."""
    frame = bytes(frame)
    if len(frame) < HEADER_BYTES:
        raise ValueError("response is shorter than its fixed header")
    magic, major, minor, header_bytes, opcode, flags, status, length, correlation = (
        struct.unpack_from("<4sHHHHIIIQ", frame)
    )
    if magic != b"TLV2" or (major, minor, header_bytes) != (2, 0, HEADER_BYTES):
        raise ValueError("response has an incompatible v2 header")
    if flags != FLAG_RESPONSE or HEADER_BYTES + length != len(frame):
        raise ValueError("response flags or payload length are not canonical")
    return ResponseHeader(opcode, flags, status, length, correlation)


def parse_handle_response(frame: bytes) -> int:
    """Return the sole Handle value from a successful response."""
    header = parse_response(frame)
    if header.status != STATUS_OK or header.payload_length != 16:
        raise ValueError("response is not a successful single-handle response")
    tag, flags, length, handle = struct.unpack_from("<HHIQ", frame, HEADER_BYTES)
    if (tag, flags, length) != (VALUE_HANDLE, 0, 0) or handle == 0:
        raise ValueError("response payload is not a canonical Handle value")
    return handle


class Runtime:
    """One process-local TinyLang runtime; use as a context manager."""

    def __init__(self, library_path: str | Path):
        self._library = ctypes.CDLL(str(Path(library_path).resolve()))
        self._configure_signatures()
        if self._library.tinylang_v2_abi_version() != ABI_VERSION:
            raise RuntimeError("TinyLang shared library has an incompatible ABI version")
        handle = ctypes.c_uint64()
        status = self._library.tinylang_v2_runtime_create(ctypes.byref(handle))
        if status != STATUS_OK or handle.value == 0:
            raise RuntimeError(f"runtime creation failed with status {status}")
        self._handle = handle.value

    def _configure_signatures(self) -> None:
        library = self._library
        library.tinylang_v2_abi_version.argtypes = []
        library.tinylang_v2_abi_version.restype = ctypes.c_uint32
        library.tinylang_v2_runtime_create.argtypes = [ctypes.POINTER(ctypes.c_uint64)]
        library.tinylang_v2_runtime_create.restype = ctypes.c_uint32
        library.tinylang_v2_runtime_destroy.argtypes = [ctypes.c_uint64]
        library.tinylang_v2_runtime_destroy.restype = ctypes.c_uint32
        library.tinylang_v2_call.argtypes = [
            ctypes.c_uint64,
            TinyBuffer,
            TinyMutableBuffer,
            ctypes.POINTER(ctypes.c_uint64),
        ]
        library.tinylang_v2_call.restype = ctypes.c_uint32

    def call(self, request: bytes, response_capacity: int) -> tuple[int, int, bytes]:
        """Return ``(call_status, required_or_written, response_bytes)``."""
        if self._handle == 0:
            raise RuntimeError("runtime is closed")
        if not 0 <= response_capacity <= MAX_FRAME_BYTES:
            raise ValueError("response capacity is outside the v2 frame limit")
        request = bytes(request)
        request_storage = (
            (ctypes.c_uint8 * len(request)).from_buffer_copy(request) if request else None
        )
        response_storage = (
            (ctypes.c_uint8 * response_capacity)() if response_capacity else None
        )
        request_pointer = (
            ctypes.cast(request_storage, _U8_POINTER) if request_storage is not None else None
        )
        response_pointer = (
            ctypes.cast(response_storage, _U8_POINTER)
            if response_storage is not None
            else None
        )
        written = ctypes.c_uint64()
        status = self._library.tinylang_v2_call(
            self._handle,
            TinyBuffer(request_pointer, len(request)),
            TinyMutableBuffer(response_pointer, response_capacity),
            ctypes.byref(written),
        )
        if written.value > response_capacity:
            response = b""
        else:
            response = (
                bytes(response_storage[: written.value])
                if response_storage is not None
                else b""
            )
        return status, written.value, response

    def close(self) -> None:
        if self._handle == 0:
            return
        status = self._library.tinylang_v2_runtime_destroy(self._handle)
        if status != STATUS_OK:
            raise RuntimeError(f"runtime destruction failed with status {status}")
        self._handle = 0

    def __enter__(self) -> Runtime:
        return self

    def __exit__(self, exc_type: object, exc_value: object, traceback: object) -> None:
        self.close()
