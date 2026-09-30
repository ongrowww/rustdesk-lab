#!/usr/bin/env python3
"""Give the Windows console its own executable and version-resource identity."""

from pathlib import Path


REPLACEMENTS = {
    Path("flutter/windows/CMakeLists.txt"): [
        ('project(ongrow_support_desk LANGUAGES CXX)', 'project(ongrow_support_console LANGUAGES CXX)'),
        ('set(BINARY_NAME "ongrow_support_desk")', 'set(BINARY_NAME "ongrow_support_console")'),
    ],
    Path("flutter/windows/runner/CMakeLists.txt"): [
        ('OUTPUT_NAME "OnGROW Support Desk"', 'OUTPUT_NAME "OnGROW Support Console"'),
    ],
    Path("flutter/windows/runner/Runner.rc"): [
        ('"OnGROW Support Desk"', '"OnGROW Support Console"'),
        ('"ongrow_support_desk"', '"ongrow_support_console"'),
        ('"OnGROW Support Desk.exe"', '"OnGROW Support Console.exe"'),
    ],
    Path("flutter/windows/runner/main.cpp"): [
        ('std::wstring app_name = L"OnGROW Support Desk";',
         'std::wstring app_name = L"OnGROW Support Console";'),
    ],
}


def apply(root: Path) -> None:
    updates = {}
    for relative, replacements in REPLACEMENTS.items():
        path = root / relative
        source = path.read_text(encoding="utf-8")
        for old, new in replacements:
            if source.count(old) < 1:
                raise ValueError(f"Windows console metadata rejected: missing {old!r} in {relative}")
            source = source.replace(old, new)
        updates[path] = source
    for path, source in updates.items():
        path.write_text(source, encoding="utf-8")


if __name__ == "__main__":
    apply(Path("."))
