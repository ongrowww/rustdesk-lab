#!/usr/bin/env python3
"""Package a verified customer Desk tree. No install, signing or update activation."""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import stat
import struct
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
DESK_EXE = "OnGROW Support Desk.exe"
TARGET = "x86_64-pc-windows-msvc"
FEATURE = "ongrow-support-desk"
REQUIRED = (DESK_EXE, "librustdesk.dll", "flutter_windows.dll", "data/icudtl.dat",
            "data/flutter_assets/AssetManifest.json", "LICENCE", "ongrow-build-provenance.txt")


def redirected(path: Path) -> bool:
    metadata = path.lstat()
    return stat.S_ISLNK(metadata.st_mode) or bool(getattr(metadata, "st_file_attributes", 0) & 0x400)


def check_parents(path: Path) -> None:
    for parent in (path, *path.parents):
        if parent.exists() or parent.is_symlink():
            if redirected(parent):
                raise ValueError("Symlink/reparse input or output path refused")


def relative_path(path: str) -> str:
    path = path.replace("\\", "/")
    if path.startswith("./"):
        path = path[2:]
    if not path or path.startswith("/") or ":" in path or not path.isascii():
        raise ValueError("Invalid Windows payload path")
    for part in path.split("/"):
        if (not part or part in (".", "..") or part.endswith((".", " "))
                or any(ord(c) < 32 or c in '<>"|?*' for c in part)):
            raise ValueError("Unsafe Windows payload component")
        base = part.split(".")[0].upper()
        if base in ("CON", "PRN", "AUX", "NUL", "CLOCK$", "CONIN$", "CONOUT$") or re.fullmatch(r"(?:COM|LPT)[1-9]", base):
            raise ValueError("Reserved Windows payload component")
    return path


def validate_desk_identity(source: Path) -> None:
    """Local role/architecture evidence; CI additionally checks actual PE resources and trust anchor."""
    binary = (source / DESK_EXE).read_bytes()
    if len(binary) < 64 or binary[:2] != b"MZ":
        raise ValueError("Expected Windows Desk PE executable")
    offset = struct.unpack_from("<I", binary, 0x3c)[0]
    if offset + 6 > len(binary) or binary[offset:offset + 4] != b"PE\0\0":
        raise ValueError("Invalid Desk PE header")
    if struct.unpack_from("<H", binary, offset + 4)[0] != 0x8664:
        raise ValueError("Expected Desk AMD64 PE")
    for identity in ("OnGROW GmbH", "OnGROW Support Desk", "ongrow_support_desk", DESK_EXE):
        if (identity + "\0").encode("utf-16le") not in binary:
            raise ValueError("Desk PE product identity missing")
    if b"OnGROW Support Desk" not in (source / "librustdesk.dll").read_bytes():
        raise ValueError("Customer role missing from Rust core")


def validate(source: Path, output: Path, version: str, source_sha: str) -> list[Path]:
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version):
        raise ValueError("Expected upstream semver version")
    if not re.fullmatch(r"[0-9a-f]{40}", source_sha):
        raise ValueError("Expected exact lowercase 40-character source SHA")
    manifest_version = re.search(r'^version = "([^"]+)"', (ROOT / "Cargo.toml").read_text(), re.M)
    if not manifest_version or manifest_version.group(1) != version:
        raise ValueError("Version differs from source manifest")
    check_parents(source)
    check_parents(output)
    if not source.is_dir():
        raise ValueError("Source directory missing")
    source = source.resolve()
    output = output.resolve()
    if source == output or source in output.parents or output in source.parents:
        raise ValueError("Source and output must not overlap")
    files = []
    names = set()
    for directory, directories, entries in os.walk(source, followlinks=False):
        for name in sorted(directories + entries):
            path = Path(directory) / name
            if redirected(path):
                raise ValueError("Symlink/reparse payload entry refused")
            relative = relative_path(path.relative_to(source).as_posix())
            folded = relative.casefold()
            if folded in names:
                raise ValueError("Case-insensitive duplicate payload entry")
            names.add(folded)
            parts = folded.split("/")
            if any(p in ("private", "secrets", "logs", "userdata") or p.startswith(".env") for p in parts):
                raise ValueError("Private/config/log payload entry refused")
            if path.is_dir():
                continue
            if not path.is_file() or path.suffix.lower() in (".pem", ".key", ".pfx", ".log", ".toml"):
                raise ValueError("Unsupported or sensitive payload entry")
            if path.suffix.lower() == ".exe" and relative != DESK_EXE:
                raise ValueError("Foreign executable in customer Desk payload")
            files.append(path)
    for required in REQUIRED:
        path = source / required
        if not path.is_file() or path.stat().st_size == 0:
            raise ValueError(f"Required Desk payload missing: {required}")
    validate_desk_identity(source)
    provenance = (source / "ongrow-build-provenance.txt").read_text(encoding="utf-8")
    if f"source={source_sha}" not in provenance.splitlines():
        raise ValueError("Build provenance source SHA mismatch")
    return sorted(files)


def generator():
    spec = importlib.util.spec_from_file_location("ongrow_portable_generator", ROOT / "libs/portable/generate.py")
    if spec is None or spec.loader is None:
        raise ValueError("Portable generator unavailable")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def package(source: Path, output: Path, version: str, source_sha: str) -> Path:
    source, output = source.absolute(), output.absolute()
    files = validate(source, output, version, source_sha)
    filename = f"ongrow-support-desk-{version}-windows-x64-{source_sha[:8]}-Setup.exe"
    targets = [output / filename, output / (filename + ".sha256"), output / (filename + ".json")]
    if any(p.exists() or p.is_symlink() for p in targets):
        raise ValueError("Setup output exists; refusing overwrite")
    # Isolate embedding/build state from both the desktop tree and the caller's checkout.
    with tempfile.TemporaryDirectory(prefix="ongrow-setup-package-") as temporary:
        staging = Path(temporary)
        packer = staging / "libs/portable"
        packer.mkdir(parents=True)
        for name in ("Cargo.toml", "Cargo.lock", "build.rs"):
            shutil.copyfile(ROOT / "libs/portable" / name, packer / name)
        shutil.copytree(ROOT / "libs/portable/src", packer / "src")
        for relative in ("res/icon.ico", "res/manifest.xml", "flutter/windows/runner/resources/app_icon.ico"):
            target = staging / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / relative, target)
        payload = staging / "payload"
        payload.mkdir()
        for file in files:
            target = payload / file.relative_to(source)
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(file, target)
        generate = generator()
        table = generate.generate_md5_table(str(payload), 11)
        generate.write_package_metadata(table, str(packer), DESK_EXE)
        generate.write_app_metadata(str(packer))
        generate.build_portable(str(packer), TARGET, FEATURE)
        binary = packer / "target" / TARGET / "release/rustdesk-portable-packer.exe"
        if not binary.is_file() or binary.stat().st_size == 0:
            raise ValueError("Compiler did not produce a Setup executable")
        digest = hashlib.sha256(binary.read_bytes()).hexdigest()
        metadata = dict(product="customer-desk", platform="windows-x64", upstream_version=version,
                        source_sha=source_sha, filename=filename, size=binary.stat().st_size,
                        sha256=digest, unsigned_lab=True)
        check_parents(output)
        output.mkdir(parents=True, exist_ok=True)
        # All outputs use exclusive create, including after a competing packaging process.
        with targets[0].open("xb") as stream, binary.open("rb") as built:
            shutil.copyfileobj(built, stream)
        with targets[1].open("x", encoding="ascii") as stream:
            stream.write(f"{digest}  {filename}\n")
        with targets[2].open("x", encoding="utf-8") as stream:
            json.dump(metadata, stream, sort_keys=True, indent=2)
            stream.write("\n")
    return targets[0]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True, type=Path, help="verified complete Desk directory")
    parser.add_argument("--output", required=True, type=Path, help="separate Setup artifact directory")
    parser.add_argument("--version", required=True)
    parser.add_argument("--source-sha", required=True)
    args = parser.parse_args()
    try:
        setup = package(args.source, args.output, args.version, args.source_sha)
    except (ValueError, OSError, RuntimeError, subprocess.CalledProcessError, ImportError) as error:
        parser.exit(1, f"Setup packaging failed: {error}\n")
    print(f"Unsigned lab Setup packaged: {setup.name}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
