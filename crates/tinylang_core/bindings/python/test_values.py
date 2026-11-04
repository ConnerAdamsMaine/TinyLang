"""Canonical wire value encoding tests."""

from __future__ import annotations

import unittest

import tinylang_v2 as sdk


class ValueEncodingTests(unittest.TestCase):
    def test_utf8_value_encodes_canonical_utf8_and_zero_padding(self) -> None:
        text = "TinyLang λ 🦀"
        encoded = sdk.utf8_value(text)
        payload = text.encode("utf-8")
        self.assertEqual(int.from_bytes(encoded[0:2], "little"), sdk.VALUE_UTF8)
        self.assertEqual(int.from_bytes(encoded[4:8], "little"), len(payload))
        self.assertEqual(encoded[16 : 16 + len(payload)], payload)
        self.assertEqual(encoded[16 + len(payload) :], bytes((-16 - len(payload)) % 8))

    def test_utf8_value_rejects_non_strings_and_unpaired_surrogates(self) -> None:
        with self.assertRaises(TypeError):
            sdk.utf8_value(42)
        with self.assertRaises(UnicodeEncodeError):
            sdk.utf8_value("\ud800")

    def test_utf8_value_parser_decodes_text_and_rejects_noncanonical_records(self) -> None:
        encoded = sdk.utf8_value("TinyLang λ 🦀")
        self.assertEqual(sdk.parse_utf8_value(encoded), "TinyLang λ 🦀")

        wrong_tag = bytearray(sdk.utf8_value("a"))
        wrong_tag[0:2] = sdk.VALUE_BYTES.to_bytes(2, "little")
        wrong_padding = bytearray(sdk.utf8_value("a"))
        wrong_padding[-1] = 1
        invalid_text = bytearray(sdk.utf8_value("a"))
        invalid_text[16] = 0xff
        for invalid in (
            bytes(wrong_tag),
            bytes(wrong_padding),
            bytes(invalid_text),
            sdk.utf8_value("a")[:17],
            sdk.utf8_value("a") + bytes(8),
        ):
            with self.subTest(invalid=invalid):
                with self.assertRaises((UnicodeDecodeError, ValueError)):
                    sdk.parse_utf8_value(invalid)


if __name__ == "__main__":
    unittest.main()
