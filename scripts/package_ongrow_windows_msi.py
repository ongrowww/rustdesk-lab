#!/usr/bin/env python3
"""Emit or compile separate OnGROW MSI packages; never install or migrate devices."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import struct
import subprocess
import uuid
import xml.etree.ElementTree as ET

from package_ongrow_windows_setup import check_parents, relative_path, redirected

ROOT = Path(__file__).resolve().parents[1]
NS = "http://wixtoolset.org/schemas/v4/wxs"
ET.register_namespace("", NS)
# Public, deterministic package identities, not device or user identifiers.
IDENTITY_NAMESPACE = uuid.uuid5(uuid.NAMESPACE_URL, "https://ongrow.de/software/msi/v1")
MAX_SEQUENCE = 256 * 256 * 65536 - 1
SNAPSHOT_FIELDS = ("st_dev", "st_ino", "st_size", "st_mtime_ns", "st_ctime_ns")
PROFILES = {
    "customer-desk": dict(name="OnGROW Support Desk", internal="ongrow_support_desk", scope="perMachine"),
    "support-console": dict(name="OnGROW Support Console", internal="ongrow_support_console", scope="perUser"),
}


def version(sequence: int) -> str:
    if isinstance(sequence, bool) or not isinstance(sequence, int) or not 1 <= sequence <= MAX_SEQUENCE:
        raise ValueError("Release sequence outside MSI version range")
    return f"{sequence // 16777216}.{(sequence // 65536) % 256}.{sequence % 65536}"


def identity(product: str, purpose: str, probe: bool = False) -> str:
    return str(uuid.uuid5(IDENTITY_NAMESPACE, f"{'probe' if probe else 'product'}/{product}/{purpose.casefold()}" )).upper()


def xml(parent, tag: str, **attrs):
    return ET.SubElement(parent, f"{{{NS}}}{tag}", {k: str(v) for k, v in attrs.items()})


def identifier(prefix: str, path: str) -> str:
    return prefix + hashlib.sha256(path.casefold().encode("ascii")).hexdigest()[:32]


def profile(product: str, probe: bool):
    if product not in PROFILES:
        raise ValueError("Unknown product")
    p = dict(PROFILES[product])
    if probe:
        p["name"] = "OnGROW MSI Probe Desk" if product == "customer-desk" else "OnGROW MSI Probe Console"
        p["internal"] = "ongrow_msi_probe"
    p["exe"] = p["name"] + ".exe"
    p["uri"] = "ongrow-msi-probe-console" if probe else "ongrow-support-console"
    p["registry"] = ("Software\\OnGROW\\MSIProbe\\" if probe else "Software\\OnGROW\\Packages\\") + product
    return p


def pe(path: Path, dll: bool):
    data = path.read_bytes()
    if len(data) < 64 or data[:2] != b"MZ":
        raise ValueError("Expected PE file")
    offset = struct.unpack_from("<I", data, 0x3c)[0]
    if offset + 24 > len(data) or data[offset:offset + 4] != b"PE\0\0":
        raise ValueError("Invalid PE header")
    if struct.unpack_from("<H", data, offset + 4)[0] != 0x8664:
        raise ValueError("Expected AMD64 payload")
    if bool(struct.unpack_from("<H", data, offset + 22)[0] & 0x2000) != dll:
        raise ValueError("Wrong PE executable/DLL role")
    return data


def snapshot(path: Path):
    s = path.lstat()
    if redirected(path) or not stat.S_ISREG(s.st_mode) or s.st_nlink != 1:
        raise ValueError("Redirected, hardlinked or nonregular payload refused")
    return (s.st_dev, s.st_ino, s.st_size, s.st_mtime_ns, s.st_ctime_ns)


def differing_fields(expected, actual):
    """Report field names only, never filesystem identities, values or paths."""
    return ",".join(name for name, before, after in zip(SNAPSHOT_FIELDS, expected, actual) if before != after)


def validate(source: Path, output: Path, product: str, sequence: int, upstream: str, sha: str, probe=False):
    version(sequence)
    p = profile(product, probe)
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", upstream) or not re.fullmatch(r"[0-9a-f]{40}", sha):
        raise ValueError("Invalid upstream version/source revision")
    for path in (source, output):
        if any(part.casefold() in ("private", "secrets") or part.casefold().startswith(".env") for part in path.parts):
            raise ValueError("Sensitive source/output ancestry refused")
    check_parents(source)
    check_parents(output)
    if not source.is_dir():
        raise ValueError("Missing source directory")
    source, output = source.resolve(), output.resolve()
    if source == output or source in output.parents or output in source.parents:
        raise ValueError("Source/output overlap")
    if output.exists():
        raise ValueError("Output must be a new exclusive directory")
    names, files = set(), []
    for directory, dirs, entries in os.walk(source, followlinks=False):
        for name in sorted(dirs + entries):
            path = Path(directory) / name
            if redirected(path):
                raise ValueError("Symlink/reparse entry refused")
            rel = relative_path(path.relative_to(source).as_posix())
            fold = rel.casefold()
            if fold in names:
                raise ValueError("Case-insensitive path alias refused")
            names.add(fold)
            if any(part in ("private", "secrets", "config", "logs", "userdata") or part.startswith(".env") for part in fold.split("/")):
                raise ValueError("Private/config/log path refused")
            if path.is_dir():
                continue
            state = snapshot(path)
            if path.suffix.lower() in (".pem", ".key", ".pfx", ".p12", ".toml", ".log", ".ini", ".config", ".db", ".sqlite", ".sqlite3"):
                raise ValueError("Sensitive payload suffix refused")
            if path.suffix.lower() == ".exe" and rel != p["exe"]:
                raise ValueError("Foreign executable refused")
            files.append((rel, path, state))
    required = [p["exe"], "probe-version.txt"] if probe else [p["exe"], "librustdesk.dll", "flutter_windows.dll", "data/icudtl.dat", "data/flutter_assets/AssetManifest.json", "LICENCE", "ongrow-build-provenance.txt"]
    for rel in required:
        if not (source / rel).is_file() or (source / rel).stat().st_size == 0:
            raise ValueError("Required runtime/data/license/provenance missing")
    binary = pe(source / p["exe"], False)
    identities = [p["name"], "ONGROW_MSI_PROBE_NO_NETWORK"] if probe else ["OnGROW GmbH", p["name"], p["internal"], p["exe"]]
    if any((value + "\0").encode("utf-16le") not in binary for value in identities):
        raise ValueError("PE product/role identity missing")
    if not probe:
        core = pe(source / "librustdesk.dll", True)
        pe(source / "flutter_windows.dll", True)
        if p["name"].encode("ascii") not in core:
            raise ValueError("Rust core product mismatch")
        if f"source={sha}" not in (source / "ongrow-build-provenance.txt").read_text(encoding="utf-8").splitlines():
            raise ValueError("Source provenance mismatch")
    elif set(rel for rel, _, _ in files) != set(required):
        raise ValueError("Probe accepts only the fake executable and version sentinel")
    # Recheck files inspected above. No output creation has happened yet.
    if any(snapshot(path) != state for _, path, state in files):
        raise ValueError("Source changed during validation")
    return p, sorted(files)


def generate(product, sequence, upstream, sha, files, probe=False):
    p = profile(product, probe)
    root = ET.Element(f"{{{NS}}}Wix")
    pkg = xml(root, "Package", Name=p["name"], Manufacturer="OnGROW GmbH", Version=version(sequence),
              ProductCode=identity(product, f"release/{sequence}/{sha}", probe),
              UpgradeCode=identity(product, "upgrade", probe), Scope=p["scope"], Language="1033", InstallerVersion="500")
    xml(pkg, "MediaTemplate", EmbedCab="yes", CompressionLevel="high")
    xml(pkg, "MajorUpgrade", Schedule="afterInstallInitialize", DowngradeErrorMessage="A newer OnGROW version is already installed.")
    exact = xml(pkg, "Upgrade", Id=identity(product, "upgrade", probe))
    xml(exact, "UpgradeVersion", Minimum=version(sequence), Maximum=version(sequence), IncludeMinimum="yes",
        IncludeMaximum="yes", OnlyDetect="yes", Property="SAME_VERSION_PRODUCT")
    xml(pkg, "Launch", Condition="Installed OR NOT SAME_VERSION_PRODUCT", Message="This release is already installed under another package identity.")
    xml(pkg, "Property", Id="ARPNOMODIFY", Value="1")
    xml(pkg, "Property", Id="MSIRESTARTMANAGERCONTROL", Value="Disable")
    # AppSearch/LaunchConditions run in both UI and execute sequences, also with /qn.
    prop = xml(pkg, "Property", Id="LEGACY_PAYLOAD", Secure="yes")
    path = "[ProgramFiles64Folder]" + p["name"] if product == "customer-desk" else "[LocalAppDataFolder]Programs\\OnGROW\\" + ("MSI Probe Console" if probe else "Support Console")
    search = xml(prop, "DirectorySearch", Id="LegacyPayloadDirectory", Path=path, Depth="0")
    xml(search, "FileSearch", Id="LegacyPayloadExe", Name=p["exe"])
    oldreg = xml(pkg, "Property", Id="LEGACY_UNINSTALL", Secure="yes")
    legacy_name = "OnGROWSupportConsole" if product == "support-console" and not probe else p["name"]
    xml(oldreg, "RegistrySearch", Id="LegacyUninstallSearch", Root="HKLM" if product == "customer-desk" else "HKCU",
        Key="Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\" + legacy_name, Name="UninstallString", Type="raw", Bitness="always64")
    extra = xml(pkg, "Property", Id="LEGACY_REGISTRATION", Secure="yes")
    extra_key = "SYSTEM\\CurrentControlSet\\Services\\" + p["name"] if product == "customer-desk" else "Software\\Classes\\" + p["uri"] + "\\shell\\open\\command"
    attrs = dict(Id="LegacyRegistrationSearch", Root="HKLM" if product == "customer-desk" else "HKCU", Key=extra_key, Type="raw", Bitness="always64")
    if product == "customer-desk": attrs["Name"] = "ImagePath"
    xml(extra, "RegistrySearch", **attrs)
    xml(pkg, "Launch", Condition="Installed OR WIX_UPGRADE_DETECTED OR (NOT LEGACY_PAYLOAD AND NOT LEGACY_UNINSTALL AND NOT LEGACY_REGISTRATION)",
        Message="An existing non-MSI installation requires a separate verified migration.")
    # Rollback is required, including on machines with a system policy disabling it.
    xml(pkg, "Launch", Condition="NOT RollbackDisabled", Message="Windows Installer rollback must be enabled.")
    dirs = {}
    if product == "customer-desk":
        base = xml(pkg, "StandardDirectory", Id="ProgramFiles64Folder")
        install = xml(base, "Directory", Id="INSTALLFOLDER", Name=p["name"])
    else:
        base = xml(pkg, "StandardDirectory", Id="LocalAppDataFolder")
        programs = xml(base, "Directory", Id="ProgramsFolder", Name="Programs")
        ongrow = xml(programs, "Directory", Id="OnGrowFolder", Name="OnGROW")
        install = xml(ongrow, "Directory", Id="INSTALLFOLDER", Name="MSI Probe Console" if probe else "Support Console")
    dirs[""] = install
    refs = []
    for rel in files:
        pieces = rel.split("/")
        for i in range(1, len(pieces)):
            parent = "/".join(pieces[:i-1]); key = "/".join(pieces[:i])
            if key not in dirs:
                dirs[key] = xml(dirs[parent], "Directory", Id=identifier("d", key), Name=pieces[i-1])
        cid, fid = identifier("c", rel), identifier("f", rel)
        comp = xml(dirs["/".join(pieces[:-1])], "Component", Id=cid, Guid=identity(product, "file/" + rel, probe), Bitness="always64")
        xml(comp, "File", Id=fid, Name=pieces[-1], Source="payload/" + rel, KeyPath="yes" if product == "customer-desk" else "no")
        if product == "support-console":
            # Per-user components have HKCU registry KeyPaths (MSI ICE38).
            xml(comp, "RegistryValue", Root="HKCU", Key=p["registry"] + "\\Components", Name=cid,
                Type="integer", Value="1", KeyPath="yes")
        refs.append(cid)
        if product == "customer-desk" and rel == p["exe"]:
            xml(comp, "ServiceInstall", Id="OwnServiceInstall", Name=p["name"], DisplayName=p["name"], Type="ownProcess",
                Start="auto", ErrorControl="normal", Account="LocalSystem", Arguments="--service", Vital="yes")
            xml(comp, "ServiceControl", Id="OwnServiceControl", Name=p["name"], Start="install", Stop="both", Remove="uninstall", Wait="yes")
    registry = xml(install, "Component", Id="OwnPackageRegistry", Guid=identity(product, "package-registry", probe), Bitness="always64")
    hive = "HKLM" if product == "customer-desk" else "HKCU"
    xml(registry, "RegistryValue", Root=hive, Key=p["registry"], Name="Sequence", Type="string", Value=str(sequence), KeyPath="yes")
    xml(registry, "RegistryValue", Root=hive, Key=p["registry"], Name="Version", Type="string", Value=version(sequence))
    xml(registry, "RegistryValue", Root=hive, Key=p["registry"], Name="Source", Type="string", Value=sha)
    xml(registry, "RegistryValue", Root=hive, Key=p["registry"], Name="UpstreamVersion", Type="string", Value=upstream)
    xml(registry, "RegistryValue", Root=hive, Key=p["registry"], Name="InstallDirectory", Type="string", Value="[INSTALLFOLDER]")
    if product == "customer-desk" and probe:
        # Owned only by the isolated fake-service profile. OnStart overwrites this
        # with the loaded assembly version, not the executable version on disk.
        xml(registry, "RegistryValue", Root=hive, Key=p["registry"], Name="RunningVersion", Type="string", Value="not-started")
    refs.append("OwnPackageRegistry")
    if product == "support-console":
        # Remove only empty owned directories, never recursive appdata cleanup.
        # Shared Programs/OnGROW ancestors deliberately remain unowned.
        for key, directory in dirs.items():
            xml(registry, "RemoveFolder", Id=identifier("r", key or "install"), Directory=directory.attrib["Id"], On="uninstall")
        uri = xml(install, "Component", Id="OwnConsoleUri", Guid=identity(product, "uri", probe), Bitness="always64")
        key = "Software\\Classes\\" + p["uri"]
        xml(uri, "RegistryValue", Root="HKCU", Key=key, Name="URL Protocol", Type="string", Value="", KeyPath="yes")
        xml(uri, "RegistryValue", Root="HKCU", Key=key + "\\shell\\open\\command", Type="string", Value='"[INSTALLFOLDER]' + p["exe"] + '" "%1"')
        refs.append("OwnConsoleUri")
    if probe:
        xml(pkg, "Property", Id="PROBE_FAIL", Secure="yes")
        xml(pkg, "CustomAction", Id="ProbeFailAfterWrite", FileRef=identifier("f", p["exe"]), ExeCommand="--fail-update",
            Execute="deferred", Impersonate="no" if product == "customer-desk" else "yes", Return="check")
        sequence_node = xml(pkg, "InstallExecuteSequence")
        xml(sequence_node, "Custom", Action="ProbeFailAfterWrite", After="StartServices", Condition='PROBE_FAIL = "1" AND NOT REMOVE')
    feature = xml(pkg, "Feature", Id="ProductFiles", Level="1")
    for ref in refs:
        xml(feature, "ComponentRef", Id=ref)
    ET.indent(root, space="  ")
    return ET.tostring(root, encoding="utf-8", xml_declaration=True) + b"\n"


def package(source, output, product, sequence, upstream, sha, emit_only=False, probe=False):
    source, output = source.absolute(), output.absolute()
    _, files = validate(source, output, product, sequence, upstream, sha, probe)
    generated = generate(product, sequence, upstream, sha, [r for r, _, _ in files], probe)
    check_parents(output)
    # No cleanup of foreign data on any failure. The incomplete exclusive output is diagnostic.
    output.mkdir()  # Parent must already exist; never create arbitrary ancestor paths.
    payload = output / "payload"
    payload.mkdir()
    hashes = {}
    for rel, src, expected in files:
        target = payload / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        if snapshot(src) != expected:
            raise ValueError("Source changed before copy")
        with src.open("rb") as inp, target.open("xb") as out:
            s = os.fstat(inp.fileno())
            opened = (s.st_dev, s.st_ino, s.st_size, s.st_mtime_ns, s.st_ctime_ns)
            if opened != expected:
                raise ValueError("Source redirected before open (differing fields: " + differing_fields(expected, opened) + ")")
            digest = hashlib.sha256()
            while chunk := inp.read(1024 * 1024):
                digest.update(chunk)
                out.write(chunk)
            s = os.fstat(inp.fileno())
            if (s.st_dev, s.st_ino, s.st_size, s.st_mtime_ns, s.st_ctime_ns) != expected or snapshot(src) != expected:
                raise ValueError("Source changed during copy")
        hashes[rel] = digest.hexdigest()
    with (output / "Package.wxs").open("xb") as out:
        out.write(generated)
    with (output / "Package.wixproj").open("xb") as out:
        out.write((ROOT / "res/msi/ongrow/Package.wixproj").read_bytes())
    if not emit_only:
        subprocess.run(["dotnet", "build", "Package.wixproj", "--configuration", "Release", "--nologo"], cwd=output, check=True)
        compiled = output / "bin/Release/OnGROW.msi"
        if not compiled.is_file() or compiled.stat().st_size == 0:
            raise ValueError("WiX did not produce the expected package")
    # Completion descriptor comes last; incomplete outputs must not be published.
    with (output / "package.json").open("x", encoding="utf-8") as out:
        json.dump(dict(product=product, probe_only=probe, sequence=sequence, msi_version=version(sequence),
                       upstream_version=upstream, source_sha=sha, payload_sha256=hashes,
                       compiled=not emit_only), out, sort_keys=True, indent=2)
        out.write("\n")
    return output


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--product", required=True, choices=sorted(PROFILES))
    parser.add_argument("--sequence", required=True, type=int)
    parser.add_argument("--upstream-version", required=True)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--emit-only", action="store_true")
    parser.add_argument("--ci-probe", action="store_true", help="separate network-free probe identities; never distribute to customers")
    args = parser.parse_args()
    try:
        package(args.source, args.output, args.product, args.sequence, args.upstream_version, args.source_sha, args.emit_only, args.ci_probe)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"MSI packaging refused: {type(error).__name__}\n")
    print("MSI package project emitted" if args.emit_only else "MSI package compiled")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
