import subprocess
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
TOOL = ROOT / "tools" / "abi_manifest.py"

DECLARED_GROUPS = ("v1-json", "v2-binary", "ralloc")
# Real CDyLib artifacts produced by `cargo build -p tynra_core -p tynra_ralloc`.
BUILT_ARTIFACTS = {
    "v1-json": ROOT / "target" / "debug" / "tynra.dll",
    "v2-binary": ROOT / "target" / "debug" / "tynra.dll",
    "ralloc": ROOT / "target" / "debug" / "ralloc.dll",
}


class AbiManifestToolTests(unittest.TestCase):
    def run_tool(self, *args: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, str(TOOL), *args],
            cwd=ROOT,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )

    def test_manifest_lists_symbols_from_rust_and_header(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            ffi = Path(tmp) / "ffi.rs"
            header = Path(tmp) / "tynra.h"
            ffi.write_text(
                textwrap.dedent(
                    """
                    #[unsafe(no_mangle)]
                    pub unsafe extern "C" fn tynra_beta(value: *const c_char) -> *mut c_char {
                        todo!()
                    }

                    #[unsafe(no_mangle)]
                    pub extern "C" fn tynra_alpha() {
                        todo!()
                    }
                    """
                ),
                encoding="utf-8",
            )
            header.write_text(
                textwrap.dedent(
                    """
                    #ifndef TYNRA_H
                    #define TYNRA_H
                    void tynra_alpha(void);
                    char *tynra_beta(const char *value);
                    #endif
                    """
                ),
                encoding="utf-8",
            )

            result = self.run_tool(
                "manifest",
                "--ffi",
                str(ffi),
                "--header",
                str(header),
            )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("rust_symbols:", result.stdout)
        self.assertIn("  - tynra_alpha", result.stdout)
        self.assertIn("  - tynra_beta", result.stdout)
        self.assertIn("header_symbols:", result.stdout)

    def test_default_check_covers_all_declared_groups(self) -> None:
        result = self.run_tool("check")

        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        for group in DECLARED_GROUPS:
            self.assertIn(f"{group}:", result.stdout)

    def test_default_manifest_reports_per_group_drift_sections(self) -> None:
        result = self.run_tool("manifest")

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("format: tynra-abi-symbol-manifest-v2", result.stdout)
        for group in DECLARED_GROUPS:
            self.assertIn(f"  {group}:", result.stdout)
            self.assertIn("  missing_from_header:", result.stdout)
            self.assertIn("  missing_from_rust_exports:", result.stdout)

    def test_check_reports_header_drift(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            ffi = Path(tmp) / "ffi.rs"
            header = Path(tmp) / "tynra.h"
            ffi.write_text(
                textwrap.dedent(
                    """
                    #[unsafe(no_mangle)]
                    pub unsafe extern "C" fn tynra_only_in_rust() {
                        todo!()
                    }
                    """
                ),
                encoding="utf-8",
            )
            header.write_text("void tynra_only_in_header(void);\n", encoding="utf-8")

            result = self.run_tool(
                "check",
                "--ffi",
                str(ffi),
                "--header",
                str(header),
            )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("missing from header", result.stdout)
        self.assertIn("tynra_only_in_rust", result.stdout)
        self.assertIn("missing from Rust exports", result.stdout)
        self.assertIn("tynra_only_in_header", result.stdout)

    def test_exports_rejects_missing_and_non_artifact_files(self) -> None:
        missing = self.run_tool("exports", str(Path("does") / "not" / "exist.dll"))
        self.assertEqual(missing.returncode, 2)
        self.assertIn("does not exist", missing.stderr)

        with tempfile.TemporaryDirectory() as tmp:
            bogus = Path(tmp) / "bogus.dll"
            bogus.write_bytes(b"not a PE or ELF image at all\n")
            result = self.run_tool("exports", str(bogus))

        self.assertEqual(result.returncode, 2)
        self.assertIn("unsupported artifact format", result.stderr)

    def test_exports_verifies_real_built_cdylib_against_declared_groups(self) -> None:
        # The tynra-v2 CDyLib statically links ralloc, so its export table is
        # the union of all three declared ABI groups; the standalone ralloc
        # CDyLib exports exactly the ralloc group.
        cases = {
            "tynra.dll": "v1-json,v2-binary,ralloc",
            "ralloc.dll": "ralloc",
        }
        for artifact_name, groups in cases.items():
            artifact = ROOT / "target" / "debug" / artifact_name
            with self.subTest(artifact=artifact_name, groups=groups):
                if not artifact.is_file():
                    self.skipTest(f"built artifact not present: {artifact}")
                result = self.run_tool("exports", str(artifact), "--expect-group", groups)

                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertIn(f"match ABI groups {groups}", result.stdout)

    def test_exports_fails_closed_on_group_mismatch(self) -> None:
        artifact = BUILT_ARTIFACTS["ralloc"]
        if not artifact.is_file():
            self.skipTest(f"built artifact not present: {artifact}")
        result = self.run_tool("exports", str(artifact), "--expect-group", "v1-json")

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("does not match ABI groups", result.stderr)

    def test_exports_rejects_unknown_group_names(self) -> None:
        artifact = BUILT_ARTIFACTS["ralloc"]
        if not artifact.is_file():
            self.skipTest(f"built artifact not present: {artifact}")
        result = self.run_tool("exports", str(artifact), "--expect-group", "no-such-group")

        self.assertEqual(result.returncode, 2)
        self.assertIn("unknown ABI group", result.stderr)

    def test_generate_header_requires_tynra_crate_manifest(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            crate = Path(tmp) / "crate"
            crate.mkdir()
            (crate / "Cargo.toml").write_text(
                textwrap.dedent(
                    """
                    [package]
                    name = "wrong-name"
                    version = "0.1.0"
                    edition = "2024"
                    """
                ),
                encoding="utf-8",
            )

            result = self.run_tool(
                "generate-header",
                "--crate-dir",
                str(crate),
                "--cbindgen",
                sys.executable,
            )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Cargo.toml", result.stderr)
        self.assertIn("tynra", result.stderr)

    def test_generate_header_checks_crate_and_passes_ffi_source_to_cbindgen(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            base = Path(tmp)
            crate = base / "Tynra"
            source_dir = crate / "src"
            source_dir.mkdir(parents=True)
            ffi_source = source_dir / "ffi.rs"
            output = base / "tynra.h"
            argv_file = base / "argv.txt"
            fake_cbindgen = base / "fake-cbindgen.py"
            (crate / "Cargo.toml").write_text(
                textwrap.dedent(
                    """
                    [package]
                    name = "tynra-v2"
                    version = "2.1.0"
                    edition = "2024"
                    """
                ),
                encoding="utf-8",
            )
            ffi_source.write_text(
                '#[unsafe(no_mangle)]\npub extern "C" fn tynra_smoke() {}\n',
                encoding="utf-8",
            )
            fake_cbindgen.write_text(
                f"#!{sys.executable}\n"
                "import sys\n"
                "from pathlib import Path\n"
                f"Path({str(argv_file)!r}).write_text("
                '"\\n".join(sys.argv[1:]), encoding="utf-8")\n',
                encoding="utf-8",
            )
            fake_cbindgen.chmod(0o755)

            result = self.run_tool(
                "generate-header",
                "--crate-dir",
                str(crate),
                "--ffi-source",
                str(ffi_source),
                "--output",
                str(output),
                "--cbindgen",
                str(fake_cbindgen),
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            argv = argv_file.read_text(encoding="utf-8").splitlines()

        self.assertNotIn("--crate", argv)
        self.assertIn("--config", argv)
        self.assertIn(str(ROOT / "cbindgen.toml"), argv)
        self.assertNotIn(str(crate), argv)
        self.assertIn(str(ffi_source), argv)
        self.assertIn("--output", argv)
        self.assertIn(str(output), argv)

    def test_generate_header_reports_missing_cbindgen_config(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            base = Path(tmp)
            crate = base / "Tynra"
            crate.mkdir()
            missing_config = base / "missing-cbindgen.toml"
            (crate / "Cargo.toml").write_text(
                textwrap.dedent(
                    """
                    [package]
                    name = "tynra-v2"
                    version = "2.1.0"
                    edition = "2024"
                    """
                ),
                encoding="utf-8",
            )

            result = self.run_tool(
                "generate-header",
                "--crate-dir",
                str(crate),
                "--config",
                str(missing_config),
                "--cbindgen",
                sys.executable,
            )

        self.assertEqual(result.returncode, 2)
        self.assertIn("cbindgen config does not exist", result.stderr)

    def test_generate_header_reports_missing_ffi_source(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            base = Path(tmp)
            crate = base / "Tynra"
            crate.mkdir()
            missing_source = base / "missing-ffi.rs"
            (crate / "Cargo.toml").write_text(
                textwrap.dedent(
                    """
                    [package]
                    name = "tynra-v2"
                    version = "2.1.0"
                    edition = "2024"
                    """
                ),
                encoding="utf-8",
            )

            result = self.run_tool(
                "generate-header",
                "--crate-dir",
                str(crate),
                "--ffi-source",
                str(missing_source),
                "--cbindgen",
                sys.executable,
            )

        self.assertEqual(result.returncode, 2)
        self.assertIn("cbindgen source does not exist", result.stderr)


if __name__ == "__main__":
    unittest.main()
