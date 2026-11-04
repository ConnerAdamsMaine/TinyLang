#!/usr/bin/env python3
"""Archive a project using .gitignore and .zipignore, without Git or dependencies.

Examples:
    python zip.py
    python zip.py /path/to/project -o /path/to/project.zip
    python zip.py . --dry-run
    python zip.py . --contents-only --level 9

Both ignore files use Git-style glob syntax (including !, /, ** and directory
patterns). Ignore files are loaded recursively. For any path, .gitignore rules
are evaluated first, then .zipignore rules, allowing .zipignore to override
.gitignore exclusions with !patterns.

As with Git, you must re-include an ignored parent directory before you can
re-include files inside it. The .git directory is always omitted. Symlinks are
not followed or archived to avoid unexpectedly including outside files.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import os
from pathlib import Path
import re
import sys
import tempfile
from typing import Iterator
import zipfile


@dataclass(frozen=True)
class IgnoreRule:
    base: tuple[str, ...]
    pattern: re.Pattern[str]
    basename_only: bool
    directories_only: bool
    exclude: bool

    def matches(self, parts: tuple[str, ...], is_dir: bool) -> bool:
        """Match this path or an ancestor directory below the rule's base."""
        base_len = len(self.base)
        if len(parts) <= base_len or parts[:base_len] != self.base:
            return False

        relative = parts[base_len:]
        for size in range(1, len(relative) + 1):
            candidate_is_dir = size < len(relative) or is_dir
            if self.directories_only and not candidate_is_dir:
                continue
            candidate = relative[size - 1] if self.basename_only else "/".join(relative[:size])
            if self.pattern.fullmatch(candidate):
                return True
        return False


def _glob_regex(glob: str) -> re.Pattern[str]:
    """Convert the most commonly used Git wildmatch constructs to regex."""
    out: list[str] = []
    i = 0
    while i < len(glob):
        char = glob[i]
        if char == "\\" and i + 1 < len(glob):
            out.append(re.escape(glob[i + 1]))
            i += 2
        elif char == "*":
            if i + 1 < len(glob) and glob[i + 1] == "*":
                before = i == 0 or glob[i - 1] == "/"
                after_index = i + 2
                after = after_index == len(glob) or glob[after_index] == "/"
                if before and after:
                    if after_index < len(glob):
                        out.append("(?:.*/)?")  # **/ matches zero or more directories
                        i += 3
                    else:
                        out.append(".*")  # trailing /**
                        i += 2
                    continue
                # Adjacent * without special ** syntax have ordinary * behavior.
                out.append("[^/]*")
                i += 2
            else:
                out.append("[^/]*")
                i += 1
        elif char == "?":
            out.append("[^/]")
            i += 1
        elif char == "[":
            j = i + 1
            if j < len(glob) and glob[j] in ("!", "^"):
                j += 1
            if j < len(glob) and glob[j] == "]":
                j += 1
            while j < len(glob) and glob[j] != "]":
                if glob[j] == "\\" and j + 1 < len(glob):
                    j += 1
                j += 1
            if j < len(glob):
                contents = glob[i + 1:j].replace("\\", "\\\\")
                if contents.startswith("!"):
                    contents = "^" + contents[1:]
                elif contents.startswith("^"):
                    contents = "\\" + contents
                try:
                    re.compile("[" + contents + "]")
                except re.error:
                    out.append(r"\[")
                    i += 1
                    continue
                out.append("(?!/)[" + contents + "]")
                i = j + 1
            else:
                out.append(r"\[")
                i += 1
        else:
            out.append(re.escape(char))
            i += 1
    return re.compile("".join(out))


def _is_escaped(value: str, index: int) -> bool:
    slash_count = 0
    index -= 1
    while index >= 0 and value[index] == "\\":
        slash_count += 1
        index -= 1
    return bool(slash_count % 2)


def _parse_ignore(path: Path, base: tuple[str, ...]) -> tuple[IgnoreRule, ...]:
    if not path.is_file():
        return ()

    rules: list[IgnoreRule] = []
    with path.open("r", encoding="utf-8-sig", errors="surrogateescape") as stream:
        for raw in stream:
            pattern = raw.rstrip("\r\n")
            while pattern.endswith(" ") and not _is_escaped(pattern, len(pattern) - 1):
                pattern = pattern[:-1]
            if not pattern or pattern.startswith("#"):
                continue

            exclude = True
            if pattern.startswith("!"):
                exclude = False
                pattern = pattern[1:]

            if not pattern:
                continue
            directories_only = pattern.endswith("/") and not _is_escaped(pattern, len(pattern) - 1)
            if directories_only:
                pattern = pattern[:-1]
            if pattern.startswith("/"):
                pattern = pattern[1:]
            if not pattern:
                continue

            rules.append(IgnoreRule(
                base=base,
                pattern=_glob_regex(pattern),
                basename_only="/" not in pattern,
                directories_only=directories_only,
                exclude=exclude,
            ))
    return tuple(rules)


def _excluded(
    parts: tuple[str, ...],
    is_dir: bool,
    git_rules: tuple[IgnoreRule, ...],
    zip_rules: tuple[IgnoreRule, ...],
) -> bool:
    excluded = False
    for rule in git_rules:
        if rule.matches(parts, is_dir):
            excluded = rule.exclude
    for rule in zip_rules:  # Always apply .zipignore after .gitignore.
        if rule.matches(parts, is_dir):
            excluded = rule.exclude
    return excluded


def _walk(
    directory: Path,
    relative: tuple[str, ...],
    git_rules: tuple[IgnoreRule, ...],
    zip_rules: tuple[IgnoreRule, ...],
    output_paths: frozenset[Path],
) -> Iterator[Path]:
    git_rules += _parse_ignore(directory / ".gitignore", relative)
    zip_rules += _parse_ignore(directory / ".zipignore", relative)

    # scandir is deliberate: symlinks are identifiable without following them.
    with os.scandir(directory) as entries:
        children = sorted(entries, key=lambda entry: entry.name)

    for child in children:
        child_path = Path(child.path)
        parts = relative + (child.name,)
        if child.name == ".git" or child_path.absolute() in output_paths or child.is_symlink():
            continue
        if child.is_dir(follow_symlinks=False):
            if _excluded(parts, True, git_rules, zip_rules):
                continue
            yield child_path  # Store directories as entries so empty directories survive.
            yield from _walk(child_path, parts, git_rules, zip_rules, output_paths)
        elif child.is_file(follow_symlinks=False):
            if not _excluded(parts, False, git_rules, zip_rules):
                yield child_path


def _file_manifest(root: Path, outputs: frozenset[Path]) -> list[Path]:
    return list(_walk(root, (), (), (), outputs))


def _archive_name(root: Path, path: Path, contents_only: bool) -> str:
    relative = path.relative_to(root).as_posix()
    return relative if contents_only else f"{root.name}/{relative}"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Archive a project respecting nested .gitignore and .zipignore files, without Git.",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument("project", nargs="?", type=Path, default=Path.cwd(), help="Project root")
    parser.add_argument("-o", "--output", type=Path, help="ZIP destination (default: alongside the project)")
    parser.add_argument("--contents-only", action="store_true", help="Do not wrap files in the project-root folder")
    parser.add_argument("--dry-run", action="store_true", help="List archive members without creating an archive")
    parser.add_argument("--level", type=int, choices=range(10), metavar="0-9", default=6,
                        help="DEFLATE compression level (0=no compression; 9=maximum)")
    args = parser.parse_args(argv)

    requested_root = args.project.expanduser().absolute()
    if requested_root.is_symlink():
        parser.error("The project root must not be a symbolic link")
    root = requested_root.resolve()
    if not root.is_dir():
        parser.error(f"Not a directory: {root}")

    output = (args.output.expanduser() if args.output is not None else root.parent / f"{root.name}.zip").absolute()
    if output.is_symlink():
        parser.error(f"Output must not be a symbolic link: {output}")
    output = output.resolve(strict=False)
    if output.is_dir():
        parser.error(f"Output must be a .zip file path, not a directory: {output}")

    temporary: Path | None = None
    try:
        # Allocate in destination directory for an atomic final rename, if writing.
        if not args.dry_run:
            output.parent.mkdir(parents=True, exist_ok=True)
            with tempfile.NamedTemporaryFile(prefix=".zip-tmp-", suffix=".zip", dir=output.parent,
                                             delete=False) as handle:
                temporary = Path(handle.name)

        excluded_outputs = frozenset({output, *([temporary] if temporary else [])})
        files = _file_manifest(root, excluded_outputs)
        regular_files = [path for path in files if path.is_file()]
        directories = len(files) - len(regular_files)
        total = sum(path.stat().st_size for path in regular_files)

        if args.dry_run:
            for path in files:
                print(_archive_name(root, path, args.contents_only) + ("/" if path.is_dir() else ""))
            print(f"\nDry run: {len(regular_files)} files, {directories} directories, "
                  f"{total:,} uncompressed bytes; nothing written.")
            return 0

        assert temporary is not None
        with zipfile.ZipFile(temporary, "w", compression=zipfile.ZIP_DEFLATED,
                             compresslevel=args.level, allowZip64=True) as archive:
            if not args.contents_only:
                archive.write(root, arcname=f"{root.name}/")
            for path in files:
                name = _archive_name(root, path, args.contents_only)
                archive.write(path, arcname=name + ("/" if path.is_dir() else ""))

        os.replace(temporary, output)
        temporary = None
        print(f"Created: {output}")
        print(f"Files: {len(regular_files)} | Directories: {directories} | "
              f"Original: {total:,} bytes | ZIP: {output.stat().st_size:,} bytes")
        return 0
    except (OSError, ValueError, zipfile.BadZipFile) as exc:
        print(f"zip.py: error: {exc}", file=sys.stderr)
        return 1
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


if __name__ == "__main__":
    raise SystemExit(main())
