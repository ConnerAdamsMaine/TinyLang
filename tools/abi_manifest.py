#!/usr/bin/env python3
"""Tynra C ABI symbol manifest, header drift, and built-artifact checker.

This tool intentionally uses only the Python standard library. The default
check compares the exported `extern "C"` Rust symbols of every advertised ABI
surface group against its committed C header:

- `v1-json`: legacy JSON entry points in
  `crates/tynra_core/src/interoperability/ffi.rs` versus
  `crates/tynra_core/include/tynra.h`.
- `v2-binary`: binary ABI entry points in
  `crates/tynra_core/src/interoperability/ffi/v2/mod.rs` versus
  `crates/tynra_core/include/tynra_v2.h`.
- `ralloc`: allocator entry points in
  `crates/tynra_ralloc/src/ralloc.rs` versus
  `crates/tynra_ralloc/include/ralloc.h`.

The `exports` command enumerates the symbols actually exported by a built
CDyLib artifact (PE or ELF), so the declared symbol set can be proven against
real binaries rather than source text alone.
"""

from __future__ import annotations

import argparse
import hashlib
import re
import struct
import pathlib
import shutil
import subprocess
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CRATES = ROOT / "crates"
DEFAULT_CBINDGEN_CONFIG = ROOT / "cbindgen.toml"
DEFAULT_GENERATED_HEADER = CRATES / "tynra_core" / "include" / "tynra.h"
EXPECTED_PACKAGE_NAME = "tynra-v2"

RUST_EXPORT_RE = re.compile(
    r"#\s*\[\s*(?:unsafe\s*\(\s*)?no_mangle\s*\)?\s*\]\s*"
    r"(?:\s*///[^\n]*\n|\s*#\[[^\]]*\]\s*)*"
    r"pub\s+(?:unsafe\s+)?extern\s+\"C\"\s+fn\s+([A-Za-z_][A-Za-z0-9_]*)",
    re.MULTILINE,
)


@dataclass(frozen=True)
class AbiGroup:
    """One advertised C ABI surface: one Rust FFI source, one C header."""

    name: str
    rust_path: Path
    header_path: Path
    symbol_prefix: str

    @property
    def header_symbol_re(self) -> re.Pattern[str]:
        return re.compile(rf"\b({re.escape(self.symbol_prefix)}_[A-Za-z0-9_]+)\s*\(")


V1_JSON_GROUP = AbiGroup(
    name="v1-json",
    rust_path=CRATES / "tynra_core" / "src" / "interoperability" / "ffi.rs",
    header_path=CRATES / "tynra_core" / "include" / "tynra.h",
    symbol_prefix="tynra",
)
V2_BINARY_GROUP = AbiGroup(
    name="v2-binary",
    rust_path=CRATES / "tynra_core" / "src" / "interoperability" / "ffi" / "v2" / "mod.rs",
    header_path=CRATES / "tynra_core" / "include" / "tynra_v2.h",
    symbol_prefix="tynra",
)
RALLOC_GROUP = AbiGroup(
    name="ralloc",
    rust_path=CRATES / "tynra_ralloc" / "src" / "ralloc.rs",
    header_path=CRATES / "tynra_ralloc" / "include" / "ralloc.h",
    symbol_prefix="ralloc",
)
ABI_GROUPS: tuple[AbiGroup, ...] = (V1_JSON_GROUP, V2_BINARY_GROUP, RALLOC_GROUP)

MANIFEST_FORMAT = "tynra-abi-symbol-manifest-v2"


@dataclass(frozen=True)
class AbiSymbols:
    group: str
    rust_symbols: tuple[str, ...]
    header_symbols: tuple[str, ...]
    rust_path: Path
    header_path: Path
    rust_sha256: str
    header_sha256: str

    @property
    def missing_from_header(self) -> tuple[str, ...]:
        return tuple(sorted(set(self.rust_symbols) - set(self.header_symbols)))

    @property
    def missing_from_rust(self) -> tuple[str, ...]:
        return tuple(sorted(set(self.header_symbols) - set(self.rust_symbols)))

    @property
    def has_drift(self) -> bool:
        return bool(self.missing_from_header or self.missing_from_rust)


class ArtifactFormatError(ValueError):
    """A built artifact could not be parsed as a PE or ELF export table."""


def read_text(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except FileNotFoundError as error:
        raise SystemExit(f"missing input file: {path}") from error


def sha256_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def strip_c_comments(text: str) -> str:
    text = re.sub(r"/\*.*?\*/", "", text, flags=re.DOTALL)
    return re.sub(r"//.*", "", text)


def rust_exports(path: Path) -> tuple[str, ...]:
    text = read_text(path)
    return tuple(sorted(set(RUST_EXPORT_RE.findall(text))))


def header_symbols(group: AbiGroup) -> tuple[str, ...]:
    text = strip_c_comments(read_text(group.header_path))
    return tuple(sorted(set(group.header_symbol_re.findall(text))))


def collect_group_symbols(group: AbiGroup) -> AbiSymbols:
    return AbiSymbols(
        group=group.name,
        rust_symbols=rust_exports(group.rust_path),
        header_symbols=header_symbols(group),
        rust_path=group.rust_path,
        header_path=group.header_path,
        rust_sha256=sha256_file(group.rust_path),
        header_sha256=sha256_file(group.header_path),
    )


def collect_symbols(
    ffi_path: Path,
    header_path: Path,
    symbol_prefix: str,
) -> AbiSymbols:
    """Collect one ad-hoc source/header pair (the legacy single-pair CLI)."""
    group = AbiGroup(
        name="custom",
        rust_path=ffi_path,
        header_path=header_path,
        symbol_prefix=symbol_prefix,
    )
    symbols = collect_group_symbols(group)
    return AbiSymbols(
        group="custom",
        rust_symbols=symbols.rust_symbols,
        header_symbols=symbols.header_symbols,
        rust_path=symbols.rust_path,
        header_path=symbols.header_path,
        rust_sha256=symbols.rust_sha256,
        header_sha256=symbols.header_sha256,
    )


def resolve_groups(
    ffi_path: Path | None,
    header_path: Path | None,
    symbol_prefix: str,
    only_group: str | None,
) -> tuple[AbiGroup, ...]:
    """Resolve which ABI groups a command covers.

    Either an explicit single FFI/header pair (legacy compatibility) or a
    selection of the declared repository ABI groups.
    """
    pair_given = ffi_path is not None or header_path is not None
    if pair_given:
        if ffi_path is None or header_path is None:
            raise SystemExit("--ffi and --header must be provided together")
        if only_group is not None:
            raise SystemExit("--group cannot be combined with --ffi/--header")
        return (AbiGroup("custom", ffi_path, header_path, symbol_prefix),)

    if only_group is None:
        return ABI_GROUPS
    matches = [group for group in ABI_GROUPS if group.name == only_group]
    if not matches:
        known = ", ".join(group.name for group in ABI_GROUPS)
        raise SystemExit(f"unknown ABI group {only_group!r}; known groups: {known}")
    return tuple(matches)


def rel(path: Path) -> str:
    try:
        return str(path.resolve().relative_to(ROOT))
    except ValueError:
        return str(path)


def yaml_list(values: tuple[str, ...], indent: str = "  ") -> list[str]:
    if not values:
        return [f"{indent}[]"]
    return [f"{indent}- {value}" for value in values]


def render_group_section(symbols: AbiSymbols, indent: str) -> list[str]:
    nested = indent + "  "
    return [
        f"{indent}inputs:",
        f"{nested}rust_ffi: {rel(symbols.rust_path)}",
        f"{nested}rust_ffi_sha256: {symbols.rust_sha256}",
        f"{nested}c_header: {rel(symbols.header_path)}",
        f"{nested}c_header_sha256: {symbols.header_sha256}",
        f"{indent}rust_symbols:",
        *yaml_list(symbols.rust_symbols, nested),
        f"{indent}header_symbols:",
        *yaml_list(symbols.header_symbols, nested),
        f"{indent}drift:",
        f"{nested}missing_from_header:",
        *yaml_list(symbols.missing_from_header, nested + "  "),
        f"{nested}missing_from_rust_exports:",
        *yaml_list(symbols.missing_from_rust, nested + "  "),
    ]


def render_manifest(symbols: list[AbiSymbols]) -> str:
    lines = [
        "# Tynra ABI symbol manifest",
        "# Deterministic symbol inventory only; not a stable ABI claim.",
        f"format: {MANIFEST_FORMAT}",
        "groups:",
    ]
    for group in symbols:
        lines.append(f"  {group.group}:")
        lines.extend(render_group_section(group, "    "))
    return "\n".join(lines) + "\n"


def validate_tynra_crate_dir(crate_dir: Path) -> Path:
    manifest = crate_dir / "Cargo.toml"
    if not manifest.is_file():
        raise ValueError(f"crate directory {rel(crate_dir)} must contain Cargo.toml")

    try:
        data = tomllib.loads(manifest.read_text(encoding="utf-8"))
    except tomllib.TOMLDecodeError as error:
        raise ValueError(f"{rel(manifest)} is not valid TOML: {error}") from error

    package = data.get("package")
    name = package.get("name") if isinstance(package, dict) else None
    if name != EXPECTED_PACKAGE_NAME:
        raise ValueError(
            f"{rel(manifest)} must be package name {EXPECTED_PACKAGE_NAME!r}; got {name!r}"
        )

    return crate_dir


def validate_cbindgen_source(source: Path) -> Path:
    if not source.is_file():
        raise ValueError(f"cbindgen source does not exist: {rel(source)}")
    return source


def command_manifest(args: argparse.Namespace) -> int:
    groups = resolve_groups(args.ffi, args.header, args.symbol_prefix, args.group)
    symbols = [collect_group_symbols(group) for group in groups]
    print(render_manifest(symbols), end="")
    return 0


def command_check(args: argparse.Namespace) -> int:
    groups = resolve_groups(args.ffi, args.header, args.symbol_prefix, args.group)
    symbols = [collect_group_symbols(group) for group in groups]
    drifted = [group for group in symbols if group.has_drift]
    if not drifted:
        summary = ", ".join(
            f"{group.group}: {len(group.rust_symbols)} symbols" for group in symbols
        )
        print(f"ABI header drift check passed ({summary}).")
        return 0

    print("ABI header drift check failed.")
    for group in drifted:
        print(f"group {group.group}:")
        if group.missing_from_header:
            print("  Rust exports missing from header:")
            for symbol in group.missing_from_header:
                print(f"    - {symbol}")
        if group.missing_from_rust:
            print("  Header declarations missing from Rust exports:")
            for symbol in group.missing_from_rust:
                print(f"    - {symbol}")
    return 1


# --- Built-artifact export tables (PE and ELF), Python standard library. ---


class _ByteReader:
    def __init__(self, data: bytes) -> None:
        self.data = data

    def u16(self, offset: int) -> int:
        return struct.unpack_from("<H", self.data, offset)[0]

    def u32(self, offset: int) -> int:
        return struct.unpack_from("<I", self.data, offset)[0]

    def u64(self, offset: int) -> int:
        return struct.unpack_from("<Q", self.data, offset)[0]

    def c_string(self, offset: int) -> str:
        end = self.data.find(b"\x00", offset)
        if end < 0:
            raise ArtifactFormatError(f"unterminated string at offset {offset}")
        raw = self.data[offset:end]
        try:
            return raw.decode("ascii")
        except UnicodeDecodeError as error:
            raise ArtifactFormatError(
                f"non-ASCII symbol name at offset {offset}"
            ) from error


def _read_pe_exports(data: bytes) -> tuple[str, ...]:
    reader = _ByteReader(data)
    if len(data) < 0x40 or data[:2] != b"MZ":
        raise ArtifactFormatError("not a PE image (missing MZ signature)")
    pe_offset = reader.u32(0x3C)
    if reader.u32(pe_offset) != 0x00004550:  # "PE\0\0"
        raise ArtifactFormatError("not a PE image (missing PE signature)")

    coff_offset = pe_offset + 4
    section_count = reader.u16(coff_offset + 2)
    optional_header_size = reader.u16(coff_offset + 16)
    optional_offset = coff_offset + 20
    optional_magic = reader.u16(optional_offset)
    # Data directories start after the fixed optional header: 96 bytes for
    # PE32, 112 bytes for PE32+.
    directories_offset = optional_offset + (96 if optional_magic == 0x10B else 112)
    export_dir_rva = reader.u32(directories_offset)
    if export_dir_rva == 0:
        return ()

    sections_offset = optional_offset + optional_header_size
    sections: list[tuple[int, int, int]] = []
    for index in range(section_count):
        entry = sections_offset + index * 40
        virtual_size = reader.u32(entry + 8)
        virtual_address = reader.u32(entry + 12)
        raw_size = reader.u32(entry + 16)
        raw_offset = reader.u32(entry + 20)
        sections.append((virtual_address, max(virtual_size, raw_size), raw_offset))

    def rva_to_offset(rva: int) -> int:
        for virtual_address, size, raw_offset in sections:
            if virtual_address <= rva < virtual_address + size:
                return rva - virtual_address + raw_offset
        raise ArtifactFormatError(f"RVA {rva:#x} is not mapped by any section")

    export_offset = rva_to_offset(export_dir_rva)
    number_of_names = reader.u32(export_offset + 24)
    names_rva = reader.u32(export_offset + 32)
    names_offset = rva_to_offset(names_rva)
    names = []
    for index in range(number_of_names):
        name_offset = rva_to_offset(reader.u32(names_offset + index * 4))
        names.append(reader.c_string(name_offset))
    return tuple(names)


def _read_elf_exports(data: bytes) -> tuple[str, ...]:
    reader = _ByteReader(data)
    if len(data) < 52 or data[:4] != b"\x7fELF":
        raise ArtifactFormatError("not an ELF image")
    if reader.u16(4) != 1 or reader.u16(5) != 1:
        raise ArtifactFormatError("only little-endian 32-bit ELF is supported")
    section_header_offset = reader.u32(0x28)
    section_entry_size = reader.u16(0x2A)
    section_count = reader.u16(0x30)
    shstr_index = reader.u16(0x32)

    def section_header(index: int) -> tuple[int, int, int]:
        offset = section_header_offset + index * section_entry_size
        return reader.u32(offset), reader.u32(offset + 4), reader.u32(offset + 16)

    shstr_sh, shstr_offset, _ = section_header(shstr_index)
    shstrtab = data[shstr_offset : shstr_offset + shstr_sh]
    sections: dict[str, tuple[int, int]] = {}
    for index in range(section_count):
        name_offset, _, header = section_header(index)
        sh_type = reader.u16(header)
        sh_offset = reader.u32(header + 4)
        sh_size = reader.u32(header + 20)
        sh_link = reader.u32(header + 24)
        sh_entsize = reader.u32(header + 32)
        end = shstrtab.find(b"\x00", name_offset)
        name = shstrtab[name_offset:end].decode("ascii", errors="replace")
        sections[name] = (sh_type, (sh_offset, sh_size, sh_link, sh_entsize))

    if ".dynsym" not in sections or ".dynstr" not in sections:
        raise ArtifactFormatError("ELF image has no dynamic symbol table")
    dynsym = sections[".dynsym"][1]
    dynstr = sections[".dynstr"][1]
    entry_size = dynsym[3] or 16
    names = []
    for offset in range(dynsym[0], dynsym[0] + dynsym[1], entry_size):
        st_name = reader.u32(offset)
        st_info = data[offset + 4]
        st_shndx = reader.u16(offset + 12)
        binding = st_info >> 4
        if st_name == 0 or st_shndx == 0 or binding not in (1, 2):
            continue
        end = data.find(b"\x00", dynstr[0] + st_name, dynstr[0] + dynstr[1])
        names.append(data[dynstr[0] + st_name : end].decode("ascii"))
    return tuple(names)


def read_artifact_exports(path: Path) -> tuple[str, ...]:
    """Enumerate the exported symbols of a built PE or ELF CDyLib."""
    data = path.read_bytes()
    if data[:2] == b"MZ":
        return _read_pe_exports(data)
    if data[:4] == b"\x7fELF":
        return _read_elf_exports(data)
    raise ArtifactFormatError("unsupported artifact format (not PE or ELF)")


def command_exports(args: argparse.Namespace) -> int:
    artifact = args.artifact
    if not artifact.is_file():
        print(f"artifact does not exist: {artifact}", file=sys.stderr)
        return 2
    try:
        exports = tuple(sorted(set(read_artifact_exports(artifact))))
    except (ArtifactFormatError, struct.error) as error:
        print(f"{rel(artifact)}: {error}", file=sys.stderr)
        return 2

    if args.expect_group is not None:
        group_names = [name.strip() for name in args.expect_group.split(",") if name.strip()]
        groups_by_name = {group.name: group for group in ABI_GROUPS}
        unknown = [name for name in group_names if name not in groups_by_name]
        if unknown:
            known = ", ".join(groups_by_name)
            print(f"unknown ABI group(s) {', '.join(unknown)!r}; known groups: {known}", file=sys.stderr)
            return 2
        declared = tuple(
            sorted({symbol for name in group_names for symbol in collect_group_symbols(groups_by_name[name]).rust_symbols})
        )
        if exports != declared:
            print(f"artifact {rel(artifact)} does not match ABI groups {args.expect_group}:", file=sys.stderr)
            for label, extra in (
                ("exported but not declared", set(exports) - set(declared)),
                ("declared but not exported", set(declared) - set(exports)),
            ):
                for symbol in sorted(extra):
                    print(f"  {label}: {symbol}", file=sys.stderr)
            return 1
        print(f"artifact exports match ABI groups {args.expect_group}: {len(exports)} symbols")
        return 0

    for symbol in exports:
        print(symbol)
    return 0


def command_generate_header(args: argparse.Namespace) -> int:
    try:
        crate_dir = validate_tynra_crate_dir(args.crate_dir)
        source = validate_cbindgen_source(args.ffi_source)
    except ValueError as error:
        print(str(error), file=sys.stderr)
        return 2

    config = args.config
    if config is not None and not config.is_file():
        print(f"cbindgen config does not exist: {rel(config)}", file=sys.stderr)
        return 2

    # Allow direct path (e.g., fake-cbindgen.py in tests) without requiring PATH lookup;
    # on Windows, `shutil.which` may not resolve `.py` files.
    cbindgen_path = pathlib.Path(args.cbindgen)
    if cbindgen_path.exists():
        cbindgen = str(cbindgen_path)
    else:
        cbindgen = shutil.which(args.cbindgen)
    if cbindgen is None:
        print(
            "cbindgen is not available on PATH; cannot generate tynra.h.",
            file=sys.stderr,
        )
        print(
            "Install cbindgen separately or run `tools/abi_manifest.py check` "
            "for the no-dependency drift check.",
            file=sys.stderr,
        )
        return 2

    output = args.output
    # On Windows, a Python fake-cbindgen (`.py`) cannot be executed directly
    # via subprocess without a `python` launcher; handle `.py` cbindgen paths
    # by prepending the current Python executable.
    if str(cbindgen).endswith(".py"):
        command = [
            str(pathlib.Path(sys.executable)),
            cbindgen,
            "--lang",
            "c",
            "--output",
            str(output),
        ]
    else:
        command = [
            cbindgen,
            "--lang",
            "c",
            "--output",
            str(output),
        ]
    if config is not None:
        command.extend(["--config", str(config)])
    command.append(str(source))
    try:
        subprocess.run(command, cwd=ROOT, check=True)
    except subprocess.CalledProcessError as error:
        return error.returncode
    print(f"generated {rel(output)} with cbindgen")
    return 0


def parser() -> argparse.ArgumentParser:
    common = argparse.ArgumentParser(add_help=False)
    common.add_argument(
        "--ffi",
        type=Path,
        default=None,
        help="Rust FFI source to inspect (with --header; default: all declared ABI groups)",
    )
    common.add_argument(
        "--header",
        type=Path,
        default=None,
        help="C header to inspect (with --ffi; default: all declared ABI groups)",
    )
    common.add_argument(
        "--symbol-prefix",
        default="tynra",
        help="header symbol prefix when using --ffi/--header (default: tynra)",
    )
    common.add_argument(
        "--group",
        choices=[group.name for group in ABI_GROUPS],
        default=None,
        help="restrict the check to one declared ABI group",
    )

    root = argparse.ArgumentParser(
        description="Check Tynra C ABI header drift, emit symbol manifests, and scan built artifacts."
    )
    subcommands = root.add_subparsers(dest="command", required=True)

    manifest = subcommands.add_parser(
        "manifest",
        parents=[common],
        help="print deterministic ABI symbol manifest",
    )
    manifest.set_defaults(func=command_manifest)

    check = subcommands.add_parser(
        "check",
        parents=[common],
        help="fail if Rust exports and header symbols drift",
    )
    check.set_defaults(func=command_check)

    exports = subcommands.add_parser(
        "exports",
        help="enumerate (or verify) the exported symbols of a built CDyLib",
    )
    exports.add_argument("artifact", type=Path, help="PE (.dll) or ELF (.so) artifact to scan")
    exports.add_argument(
        "--expect-group",
        default=None,
        metavar="GROUPS",
        help="comma-separated ABI group names; fail unless the artifact exports exactly their union",
    )
    exports.set_defaults(func=command_exports)

    generate = subcommands.add_parser(
        "generate-header",
        help="generate planned tynra.h with cbindgen when cbindgen is installed",
    )
    generate.add_argument(
        "--crate-dir",
        type=Path,
        default=CRATES / "tynra_core",
        help="Rust crate directory whose Cargo.toml must identify the tynra-v2 package",
    )
    generate.add_argument(
        "--ffi-source",
        type=Path,
        default=V1_JSON_GROUP.rust_path,
        help=f"Rust FFI source passed to cbindgen (default: {rel(V1_JSON_GROUP.rust_path)})",
    )
    generate.add_argument(
        "--output",
        type=Path,
        default=DEFAULT_GENERATED_HEADER,
        help=f"generated header path (default: {rel(DEFAULT_GENERATED_HEADER)})",
    )
    generate.add_argument(
        "--cbindgen",
        default="cbindgen",
        help="cbindgen executable name or path",
    )
    generate.add_argument(
        "--config",
        type=Path,
        default=DEFAULT_CBINDGEN_CONFIG if DEFAULT_CBINDGEN_CONFIG.exists() else None,
        help=f"cbindgen config path (default: {rel(DEFAULT_CBINDGEN_CONFIG)} when present)",
    )
    generate.set_defaults(func=command_generate_header)

    return root


def main(argv: list[str] | None = None) -> int:
    args = parser().parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
