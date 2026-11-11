"""Regression tests for explicit PROGRAM_LOAD artifact formats."""

from __future__ import annotations

import struct
import unittest

import tinylang_v2 as sdk


class ProgramLoadTests(unittest.TestCase):
    def test_default_format1_remains_byte_for_byte_compatible(self) -> None:
        request = sdk.make_program_load_request(b"\xaa\xbb\xcc", 0x0102_0304_0506_0708)
        expected = bytes.fromhex(
            "544c5632 0200 0000 2000 0010 00000000 00000000"
            "30000000 0807060504030201"
            "0400 0000 04000000 0000000000000000 01000000 00000000"
            "0c00 0000 03000000 0000000000000000 aabbcc 0000000000"
        )
        self.assertEqual(request, expected)

    def test_format2_is_explicit_and_uses_canonical_records(self) -> None:
        request = sdk.make_program_load_request(
            b"\xaa\xbb\xcc", 9, artifact_format=2
        )
        self.assertEqual(
            request[32:],
            bytes.fromhex(
                "0400 0000 04000000 0000000000000000 02000000 00000000"
                "0c00 0000 03000000 0000000000000000 aabbcc 0000000000"
            ),
        )
        self.assertEqual(struct.unpack_from("<I", request, 20)[0], 48)
        self.assertEqual(struct.unpack_from("<Q", request, 24)[0], 9)

    def test_payload_helper_matches_request_and_accepts_named_formats(self) -> None:
        for artifact_format in sdk.ArtifactFormat:
            with self.subTest(artifact_format=artifact_format):
                payload = sdk.program_load_payload(
                    b"artifact", artifact_format=artifact_format
                )
                self.assertEqual(
                    sdk.make_program_load_request(
                        b"artifact", artifact_format=artifact_format
                    )[32:],
                    payload,
                )
                self.assertEqual(struct.unpack_from("<I", payload, 16)[0], artifact_format)
        self.assertEqual(
            sdk.program_load_payload(b"artifact"),
            sdk.program_load_payload(b"artifact", artifact_format=1),
        )

    def test_rejects_invalid_format_options_without_coercion(self) -> None:
        for invalid in (0, 3, -1, 0x1_0000_0000, True, False, 1.0, 2.0, "2", None, {}):
            with self.subTest(invalid=invalid):
                with self.assertRaisesRegex(ValueError, "artifact_format"):
                    sdk.make_program_load_request(b"artifact", artifact_format=invalid)

    def test_rejects_empty_and_over_limit_artifacts(self) -> None:
        for artifact_format, limit in ((1, 16 * 1024 * 1024), (2, 8 * 1024 * 1024)):
            for artifact in (b"", bytes(limit + 1)):
                with self.subTest(artifact_format=artifact_format, length=len(artifact)):
                    with self.assertRaisesRegex(ValueError, "artifact.*length"):
                        sdk.make_program_load_request(
                            artifact, artifact_format=artifact_format
                        )

    def test_limit_and_offset_view_preserve_exact_artifact_bytes(self) -> None:
        for artifact_format, limit in ((1, 16 * 1024 * 1024), (2, 8 * 1024 * 1024)):
            with self.subTest(artifact_format=artifact_format):
                artifact = bytes(limit)
                request = sdk.make_program_load_request(
                    artifact, artifact_format=artifact_format
                )
                self.assertEqual(len(request), 72 + limit)
                self.assertEqual(struct.unpack_from("<I", request, 60)[0], limit)
                self.assertEqual(request[72:], artifact)
        request = sdk.make_program_load_request(
            memoryview(b"prefixABCtail")[6:9], artifact_format=2
        )
        self.assertEqual(struct.unpack_from("<I", request, 60)[0], 3)
        self.assertEqual(request[72:], b"ABC" + bytes(5))


if __name__ == "__main__":
    unittest.main()
