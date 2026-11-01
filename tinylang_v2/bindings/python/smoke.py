"""Run a linked ctypes smoke check against a built TinyLang v2 library."""

from __future__ import annotations

import sys

from tinylang_v2 import (
    HEADER_BYTES,
    OPCODE_NOP,
    OPCODE_VM_CREATE,
    STATUS_BUFFER_TOO_SMALL,
    STATUS_INVALID_HANDLE,
    STATUS_OK,
    Runtime,
    make_request,
    make_vm_create_request,
    parse_response,
)


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: smoke.py PATH_TO_TINYLANG_V2_LIBRARY", file=sys.stderr)
        return 2
    correlation_id = 0x0102_0304_0506_0708
    request = make_request(OPCODE_NOP, correlation_id=correlation_id)
    with Runtime(sys.argv[1]) as runtime:
        status, required, response = runtime.call(request, HEADER_BYTES - 1)
        assert (status, required, response) == (
            STATUS_BUFFER_TOO_SMALL,
            HEADER_BYTES,
            b"",
        )
        status, written, response = runtime.call(request, required)
        assert status == STATUS_OK and written == HEADER_BYTES
        header = parse_response(response)
        assert header.opcode == OPCODE_NOP
        assert header.status == STATUS_OK
        assert header.payload_length == 0
        assert header.correlation_id == correlation_id

        vm_request = make_vm_create_request(
            0x0000_0001_0000_0001,
            1024,
            32,
            8,
            correlation_id=0x5152_5354_5556_5758,
        )
        status, written, response = runtime.call(vm_request, 48)
        assert status == STATUS_OK and written == 48
        header = parse_response(response)
        assert header.opcode == OPCODE_VM_CREATE
        assert header.status == STATUS_INVALID_HANDLE
        assert header.payload_length == 16
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
