"""Load simple TLBC programs through the v2 ABI from a Python host."""

from __future__ import annotations

import sys
from pathlib import Path

from tinylang_v2 import (
    STATUS_OK,
    Runtime,
    make_handle_release_request,
    make_program_load_request,
    make_vm_create_request,
    parse_handle_response,
    parse_response,
)


def checked_call(runtime: Runtime, request: bytes, expected_length: int) -> bytes:
    call_status, written, response = runtime.call(request, max(48, expected_length))
    if call_status != STATUS_OK or written != expected_length:
        raise RuntimeError(f"raw call failed: status={call_status} written={written}")
    if parse_response(response).status != STATUS_OK:
        raise RuntimeError(f"operation failed: response_status={parse_response(response).status}")
    return response


def run_case(runtime: Runtime, path: Path, correlation: int) -> None:
    artifact = path.read_bytes()
    load = checked_call(
        runtime,
        make_program_load_request(artifact, correlation),
        48,
    )
    program = parse_handle_response(load)
    create = checked_call(
        runtime,
        make_vm_create_request(program, 1024, 32, 8, correlation_id=correlation + 1),
        48,
    )
    vm = parse_handle_response(create)

    checked_call(runtime, make_handle_release_request(program, correlation + 2), 32)
    checked_call(runtime, make_handle_release_request(vm, correlation + 3), 32)
    print(
        f"PY-TL {path.name}: load=OK vm_create=OK lifecycle=OK "
        "execute=UNAVAILABLE(VM_INVOKE)"
    )


def main() -> int:
    if len(sys.argv) < 2:
        print("usage: programs.py PATH_TO_LIBRARY [ARTIFACT ...]", file=sys.stderr)
        return 2
    artifacts = [Path(value) for value in sys.argv[2:]]
    if not artifacts:
        base = Path(__file__).resolve().parents[2] / "tests" / "artifacts"
        artifacts = [
            base / "minimal_halt.tlbc",
            base / "push_print_halt.tlbc",
            base / "arithmetic_print.tlbc",
        ]
    with Runtime(sys.argv[1]) as runtime:
        for index, artifact in enumerate(artifacts):
            run_case(runtime, artifact, 0x2000 + index * 4)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
