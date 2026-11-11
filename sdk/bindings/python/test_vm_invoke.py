"""VM_INVOKE wire request tests."""

from __future__ import annotations

import struct
import unittest

import tinylang_v2 as sdk


class VmInvokeRequestTests(unittest.TestCase):
    def test_request_uses_the_specified_sequence_schema(self) -> None:
        argument = sdk.value_record(9, struct.pack("<q", 42))
        request = sdk.make_vm_invoke_request(
            0x0000_0001_0000_0001, 3, 500, 128, [argument], 77
        )
        self.assertEqual(struct.unpack_from("<H", request, 10)[0], sdk.OPCODE_VM_INVOKE)
        self.assertEqual(struct.unpack_from("<Q", request, 24)[0], 77)
        self.assertEqual(struct.unpack_from("<I", request, 64)[0], 3)
        self.assertEqual(struct.unpack_from("<Q", request, 88)[0], 500)
        self.assertEqual(struct.unpack_from("<I", request, 112)[0], 128)
        self.assertEqual(struct.unpack_from("<H", request, 120)[0], sdk.VALUE_SEQUENCE)
        self.assertEqual(struct.unpack_from("<Q", request, 128)[0], 1)
        self.assertEqual(request[136:], argument)

    def test_request_rejects_invalid_limits_and_arguments(self) -> None:
        for vm, export_id, fuel, max_result_bytes, arguments in (
            (0, 0, 1, 24, []),
            (1, -1, 1, 24, []),
            (1, 0, 0, 24, []),
            (1, 0, 1, 16 * 1024 * 1024 + 1, []),
            (1, 0, 1, 24, None),
        ):
            with self.subTest(vm=vm, export_id=export_id, fuel=fuel):
                with self.assertRaises((TypeError, ValueError)):
                    sdk.make_vm_invoke_request(
                        vm, export_id, fuel, max_result_bytes, arguments
                    )


if __name__ == "__main__":
    unittest.main()
