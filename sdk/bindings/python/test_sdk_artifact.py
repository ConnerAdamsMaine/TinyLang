"""Real compiler artifact integration; no hand-built executable bytecode."""

from __future__ import annotations

from contextlib import ExitStack, contextmanager
from collections.abc import Iterator
import os
from pathlib import Path
import struct
import unittest

import tinylang_v2 as sdk


class CompilerArtifactIntegrationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        library = os.environ.get("TINYLANG_V2_LIBRARY")
        artifact_path = os.environ.get("TINYLANG_V2_COMPILER_ARTIFACT")
        if library is None and artifact_path is None:
            raise unittest.SkipTest("set TINYLANG_V2_LIBRARY and TINYLANG_V2_COMPILER_ARTIFACT")
        if not library or not artifact_path:
            raise ValueError("both explicit DLL and compiler artifact paths are required")
        cls.library_path = Path(library)
        cls.artifact = Path(artifact_path).read_bytes()
        if cls.artifact[:10] != b"TINYLANG\x05\x00":
            raise ValueError("expected the compiler-produced version-5 sdk_artifact output")

    def checked_call(
        self, runtime: sdk.Runtime, request: bytes, capacity: int, expected_status: int = 0
    ) -> bytes:
        status, written, response = runtime.call(request, capacity)
        self.assertEqual(status, sdk.STATUS_OK)
        self.assertEqual(written, len(response))
        header = sdk.parse_response(response)
        self.assertEqual(header.status, expected_status)
        self.assertEqual(header.opcode, struct.unpack_from("<H", request, 10)[0])
        self.assertEqual(header.correlation_id, struct.unpack_from("<Q", request, 24)[0])
        return response

    @contextmanager
    def owned_handle(self, runtime: sdk.Runtime, request: bytes) -> Iterator[int]:
        handle = sdk.parse_handle_response(self.checked_call(runtime, request, 48))
        try:
            yield handle
        finally:
            response = self.checked_call(
                runtime, sdk.make_handle_release_request(handle), 48
            )
            self.assertEqual(len(response), 32)

    def test_compiler_add_invokes_through_real_dll_and_cleans_lifecycle(self) -> None:
        with sdk.Runtime(self.library_path) as runtime, ExitStack() as handles:
            with self.owned_handle(
                runtime,
                sdk.make_program_load_request(self.artifact, 101, artifact_format=2),
            ) as program:
                vm = handles.enter_context(self.owned_handle(
                    runtime,
                    sdk.make_vm_create_request(program, 1024, 32, 8, correlation_id=102),
                ))
            # Program reference is released before invocation; VM retains its own.
            arguments = b"".join(
                sdk.value_record(9, struct.pack("<q", value)) for value in (19, 23)
            )
            payload = b"".join((
                sdk.handle_value(vm),
                sdk.u32_value(0),  # sdk_artifact generates add at function-table index 0.
                sdk.u64_value(1000),
                sdk.u32_value(24),
                sdk.value_record(17, arguments, auxiliary=2),
            ))
            request = sdk.make_request(0x1002, payload, 103)
            self.assertEqual(
                runtime.call(request, 55),
                (sdk.STATUS_BUFFER_TOO_SMALL, 56, b""),
            )
            response = self.checked_call(runtime, request, 56)
            self.assertEqual(
                response[32:],
                bytes.fromhex("0900 0000 08000000 0000000000000000 2a00000000000000"),
            )
            self.assertEqual(struct.unpack_from("<q", response, 48)[0], 42)
        with self.assertRaisesRegex(RuntimeError, "closed"):
            runtime.call(request, 56)
        print(
            f"PY compiler artifact: version=5 bytes={len(self.artifact)} "
            "format=2 DLL=OK add(19,23)=42 program_release=OK vm_release=OK "
            "runtime_destroy=OK buffers=host-owned"
        )

    def test_default_format1_does_not_fall_back_to_compiler_decoder(self) -> None:
        with sdk.Runtime(self.library_path) as runtime:
            response = self.checked_call(
                runtime, sdk.make_program_load_request(self.artifact, 201), 48, 13
            )
            self.assertEqual(len(response), 48)
            self.assertEqual(struct.unpack_from("<H", response, 32)[0], 2)  # VM error detail

    def test_format2_rejects_tlbc_and_corrupt_compiler_artifacts(self) -> None:
        tlbc = Path(__file__).resolve().parents[2] / "tests" / "artifacts" / "minimal_halt.tlbc"
        invalid_artifacts = (
            tlbc.read_bytes(),
            self.artifact + b"\x00",
            self.artifact[:8] + b"\x04\x00" + self.artifact[10:],
        )
        with sdk.Runtime(self.library_path) as runtime:
            for artifact in invalid_artifacts:
                with self.subTest(prefix=artifact[:10], length=len(artifact)):
                    self.checked_call(
                        runtime,
                        sdk.make_program_load_request(artifact, 301, artifact_format=2),
                        48,
                        13,
                    )

    def test_default_format1_still_loads_real_tlbc(self) -> None:
        path = Path(__file__).resolve().parents[2] / "tests" / "artifacts" / "minimal_halt.tlbc"
        with sdk.Runtime(self.library_path) as runtime:
            with self.owned_handle(runtime, sdk.make_program_load_request(path.read_bytes())):
                pass


if __name__ == "__main__":
    unittest.main()
