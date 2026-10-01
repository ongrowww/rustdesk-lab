#!/usr/bin/env python3
"""Create isolated signed lab bundles. No upload, client bootstrap or auto-apply."""

from __future__ import annotations

import argparse
import hashlib
import hmac
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import time
from urllib.parse import urlsplit

DOMAIN = b"OnGROW signed update manifest v1\0"
MAX_MANIFEST_BYTES = 65536
U64_MAX = (1 << 64) - 1
SPKI_PREFIX = bytes.fromhex("302a300506032b6570032100")
PRIVATE_NAME = "windows-lab-ed25519.pem"
PUBLIC_NAME = "windows-lab-ed25519.pub"
INTERNAL_NAMES = {"manifest.json", "manifest.sig", ".signing-message", ".signing-public.der"}


class SigningError(Exception):
    """Only fixed, non-secret diagnostics may reach the CLI."""


def identity(metadata):
    return metadata.st_dev, metadata.st_ino


def redirected(metadata):
    return stat.S_ISLNK(metadata.st_mode) or bool(getattr(metadata, "st_file_attributes", 0) & 0x400)


def safe_metadata(metadata, *, directory=False, private=False):
    if redirected(metadata) or not (stat.S_ISDIR(metadata.st_mode) if directory else stat.S_ISREG(metadata.st_mode)):
        raise SigningError("Redirected or non-regular path refused")
    if hasattr(os, "getuid"):
        uid = os.getuid()
        if metadata.st_uid not in (uid, 0) or (private and metadata.st_uid != uid):
            raise SigningError("Unexpected path owner")
        mode = stat.S_IMODE(metadata.st_mode)
        if mode & 0o022 or (private and mode != (0o700 if directory else 0o600)):
            raise SigningError("Unsafe path permissions")
    if not directory and metadata.st_nlink != 1:
        raise SigningError("Hardlinked file refused")


def absolute_path(value):
    raw = os.fspath(value)
    if (not raw or not Path(raw).is_absolute() or raw.startswith("//")
            or "\\" in raw or ":" in raw or "\0" in raw or ".." in raw.split("/")):
        raise SigningError("Expected an absolute unambiguous local path")
    return Path(raw)


def check_parents(path):
    for parent in reversed(path.parents):
        try:
            metadata = parent.lstat()
        except FileNotFoundError:
            continue
        safe_metadata(metadata, directory=True)


def local_path(value):
    path = absolute_path(value)
    check_parents(path)
    return path


def regular_file(path, *, private=False):
    check_parents(path)
    metadata = path.lstat()
    safe_metadata(metadata, private=private)
    if private:
        safe_metadata(path.parent.lstat(), directory=True, private=True)
    return metadata


def outside_git(path):
    if any((parent / ".git").exists() or (parent / ".git").is_symlink() for parent in path.parents):
        raise SigningError("Signing keys must be outside Git worktrees")


def overlapping(left, right):
    return left == right or left in right.parents or right in left.parents


def absent(path):
    # lexists also rejects dangling symlinks. Casefold collisions are rejected
    # on case-sensitive hosts too, before any output mutation.
    if os.path.lexists(path):
        raise SigningError("Existing output refused")
    if path.parent.exists() and any(entry.name.casefold() == path.name.casefold() for entry in path.parent.iterdir()):
        raise SigningError("Case-insensitive output collision")


class OwnedPaths:
    """Cleanup only exact inodes created by this invocation, never recursively."""

    def __init__(self):
        self.files = []
        self.directories = []

    def mkdir(self, path):
        path.mkdir(mode=0o700)
        self.directories.append((path, identity(path.lstat())))
        safe_metadata(path.lstat(), directory=True, private=True)

    def create(self, path):
        flags = os.O_CREAT | os.O_EXCL | os.O_RDWR | getattr(os, "O_NOFOLLOW", 0)
        descriptor = os.open(path, flags, 0o600)
        self.files.append((path, identity(os.fstat(descriptor))))
        return os.fdopen(descriptor, "w+b")

    def write(self, path, data):
        with self.create(path) as stream:
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())

    def remove(self, path):
        for owned, inode in self.files:
            if owned == path:
                metadata = path.lstat()
                if identity(metadata) != inode or not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1:
                    raise SigningError("Owned output changed; refusing cleanup")
                path.unlink()
                return
        raise SigningError("Unowned cleanup refused")

    def cleanup(self):
        for path, inode in reversed(self.files):
            try:
                metadata = path.lstat()
                if identity(metadata) == inode and stat.S_ISREG(metadata.st_mode) and metadata.st_nlink == 1:
                    path.unlink()
            except OSError:
                pass  # Unknown/replaced leftovers must survive.
        for path, inode in reversed(self.directories):
            try:
                metadata = path.lstat()
                if identity(metadata) == inode and stat.S_ISDIR(metadata.st_mode):
                    path.rmdir()  # Only succeeds when no unknown entries remain.
            except OSError:
                pass


def openssl_operation(tool, arguments, *, output=None):
    try:
        result = subprocess.run([str(tool), *arguments], stdin=subprocess.DEVNULL,
                                stdout=output if output is not None else subprocess.PIPE,
                                stderr=subprocess.PIPE, timeout=30, check=False)
    except (OSError, subprocess.SubprocessError):
        raise SigningError("OpenSSL operation failed") from None
    if result.returncode != 0:
        # Never attach stderr, stdout, command arguments or the exception.
        raise SigningError("OpenSSL operation failed")
    return result.stdout


def check_tool_parents(tool, requested):
    canonical_homebrew = bool(re.fullmatch(r"/opt/homebrew/Cellar/openssl@3/[0-9]+\.[0-9]+\.[0-9]+/bin/openssl", str(tool)))
    explicit_homebrew = requested == Path("/opt/homebrew/opt/openssl@3/bin/openssl") or requested == tool
    for parent in reversed(tool.parents):
        metadata = parent.lstat()
        if parent == Path("/opt/homebrew/Cellar") and canonical_homebrew and explicit_homebrew:
            # Explicitly authorized Homebrew exception, not a data/key policy.
            # We trust the local administrator and the selected toolchain.
            import grp
            try:
                admin_group = grp.getgrgid(metadata.st_gid).gr_name == "admin"
            except KeyError:
                admin_group = False
            if (not redirected(metadata) and stat.S_ISDIR(metadata.st_mode)
                    and metadata.st_uid == os.getuid()
                    and stat.S_IMODE(metadata.st_mode) == 0o775
                    and admin_group):
                continue
        safe_metadata(metadata, directory=True)


def openssl_tool(value):
    # Homebrew's explicit opt path is a symlink. Resolve only the tool,
    # then validate the actual executable and all its parents.
    requested = absolute_path(value)
    try:
        tool = requested.resolve(strict=True)
        safe_metadata(tool.lstat())
        check_tool_parents(tool, requested)
        if not os.access(tool, os.X_OK):
            raise SigningError("OpenSSL executable unavailable")
        version = openssl_operation(tool, ["version"])
    except OSError:
        raise SigningError("OpenSSL executable unavailable") from None
    if not re.match(rb"OpenSSL 3\.[0-9]+\.[0-9]+(?:\s|$)", version):
        raise SigningError("OpenSSL 3 is required")
    return tool


def public_from_private(tool, key):
    # Only public DER is captured. The private file is read by OpenSSL, never Python.
    der = openssl_operation(tool, ["pkey", "-in", str(key), "-pubout", "-outform", "DER"])
    if len(der) != len(SPKI_PREFIX) + 32 or not der.startswith(SPKI_PREFIX):
        raise SigningError("Expected exact Ed25519 public SPKI")
    return der[len(SPKI_PREFIX):]


def init_key(directory, openssl):
    tool = openssl_tool(openssl)
    directory = local_path(directory)
    outside_git(directory / PRIVATE_NAME)
    absent(directory)
    missing = []
    current = directory
    while not current.exists():
        missing.append(current)
        current = current.parent
    owned = OwnedPaths()
    try:
        for path in reversed(missing):
            owned.mkdir(path)
        key = directory / PRIVATE_NAME
        with owned.create(key) as output:
            openssl_operation(tool, ["genpkey", "-algorithm", "ED25519"], output=output)
            output.flush()
            os.fsync(output.fileno())
        regular_file(key, private=True)
        if key.stat().st_size == 0:
            raise SigningError("OpenSSL did not create a key")
        public = public_from_private(tool, key)
        owned.write(directory / PUBLIC_NAME, public)
        return hashlib.sha256(public).hexdigest()
    except Exception:
        owned.cleanup()
        raise


def unsigned64(value):
    if isinstance(value, bool) or not isinstance(value, int) or not 0 <= value <= U64_MAX:
        raise SigningError("Expected an unsigned 64-bit integer")
    return value


def plain_component(value):
    return bool(re.fullmatch(r"[A-Za-z0-9_.-]+", value)) and value not in (".", "..")


def filename(value):
    base = value.split(".")[0].upper()
    if (not plain_component(value) or len(value) > 255 or value.endswith(".")
            or base in ("CON", "PRN", "AUX", "NUL") or re.fullmatch(r"(?:COM|LPT)[1-9]", base)
            or value.casefold() in INTERNAL_NAMES):
        raise SigningError("Unsafe or conflicting payload filename")
    return value


def download_location(origin, prefix):
    # Conservative canonical DNS origins avoid url-parser normalization aliases.
    if not re.fullmatch(r"https://[a-z0-9.-]+(?::[1-9][0-9]*)?", origin):
        raise SigningError("Expected canonical HTTPS DNS origin")
    parsed = urlsplit(origin)
    labels = parsed.hostname.split(".")
    if (not any(c.isalpha() for c in parsed.hostname) or len(parsed.hostname) > 253
            or any(not re.fullmatch(r"[a-z0-9](?:[a-z0-9-]*[a-z0-9])?", label) or len(label) > 63 for label in labels)):
        raise SigningError("Expected canonical HTTPS DNS origin")
    try:
        if parsed.port is not None and (parsed.port == 443 or parsed.port > 65535):
            raise SigningError("Noncanonical HTTPS port")
    except ValueError:
        raise SigningError("Invalid HTTPS port") from None
    if (not prefix.startswith("/") or prefix.startswith("//") or not prefix.endswith("/")
            or prefix == "/" or not all(plain_component(part) for part in prefix[1:-1].split("/"))):
        raise SigningError("Unsafe download path prefix")
    return origin, prefix


def read_public(path):
    before = regular_file(path, private=True)
    descriptor = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
    with os.fdopen(descriptor, "rb") as stream:
        if identity(os.fstat(stream.fileno())) != identity(before):
            raise SigningError("Public key changed")
        public = stream.read(33)
    if len(public) != 32:
        raise SigningError("Expected raw 32-byte public key")
    return public


def source_state(metadata):
    return identity(metadata), metadata.st_size, metadata.st_mtime_ns, metadata.st_ctime_ns


def copy_payload(source, destination, owned):
    before = regular_file(source)
    descriptor = os.open(source, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
    with os.fdopen(descriptor, "rb") as original, owned.create(destination) as copied:
        if source_state(os.fstat(original.fileno())) != source_state(before):
            raise SigningError("Payload source changed")
        while True:
            block = original.read(1024 * 1024)
            if not block:
                break
            copied.write(block)
        copied.flush()
        os.fsync(copied.fileno())
        if (source_state(os.fstat(original.fileno())) != source_state(before)
                or source_state(regular_file(source)) != source_state(before)):
            raise SigningError("Payload source changed")
        # Hash the copied file, not a cached digest of the source.
        copied.seek(0)
        digest = hashlib.sha256()
        size = 0
        while True:
            block = copied.read(1024 * 1024)
            if not block:
                break
            digest.update(block)
            size += len(block)
        if size != before.st_size or size == 0 or size > U64_MAX:
            raise SigningError("Invalid copied payload size")
        return size, digest.hexdigest()


def manifest_bytes(manifest):
    raw = (json.dumps(manifest, ensure_ascii=True, sort_keys=True, separators=(",", ":")) + "\n").encode("utf-8")
    if len(raw) > MAX_MANIFEST_BYTES:
        raise SigningError("Manifest exceeds schema limit")
    return raw


def sign_release(*, payload, key_file, public_key_file, openssl, output_dir, product,
                 platform, channel, release_sequence, previous_sequence, version,
                 source_sha, issued_at, expires_at, origin, path_prefix, now=None):
    tool = openssl_tool(openssl)
    if product not in ("customer-desk", "support-console") or platform != "windows-x64" or channel != "lab":
        raise SigningError("Only the two Windows x64 lab products are supported")
    release_sequence, previous_sequence = unsigned64(release_sequence), unsigned64(previous_sequence)
    issued_at, expires_at = unsigned64(issued_at), unsigned64(expires_at)
    now = unsigned64(int(time.time()) if now is None else now)
    if release_sequence <= previous_sequence:
        raise SigningError("Release sequence must increase")
    if issued_at > now or expires_at <= now or expires_at <= issued_at:
        raise SigningError("Invalid release validity interval")
    if not re.fullmatch(r"[A-Za-z0-9_.-]{1,64}", version) or not re.fullmatch(r"[0-9a-f]{40}", source_sha):
        raise SigningError("Invalid version or source revision")
    origin, path_prefix = download_location(origin, path_prefix)
    source, output = local_path(payload), local_path(output_dir)
    key, public_path = local_path(key_file), local_path(public_key_file)
    name = filename(source.name)
    if source.suffix.lower() not in (".exe", ".zip", ".msi"):
        raise SigningError("Expected an existing lab EXE, ZIP or MSI payload")
    if any(part.lower() in ("private", "secrets") or part.lower().startswith(".env") for part in source.parts):
        raise SigningError("Sensitive payload path refused")
    if sum(entry.name.casefold() == name.casefold() for entry in source.parent.iterdir()) != 1:
        raise SigningError("Case-insensitive payload collision")
    source_metadata = regular_file(source)
    if source_metadata.st_size == 0 or source_metadata.st_size > U64_MAX:
        raise SigningError("Invalid payload size")
    absent(output)
    safe_metadata(output.parent.lstat(), directory=True)
    if overlapping(source, output):
        raise SigningError("Payload source and output overlap")
    for path in (key, public_path):
        outside_git(path)
        regular_file(path, private=True)
        if overlapping(path, output) or path == source:
            raise SigningError("Signing keys overlap payload or output")
    public = read_public(public_path)
    if not hmac.compare_digest(public_from_private(tool, key), public):
        raise SigningError("Private and public keys do not match")
    manifest = dict(schema_version=1, product=product, platform=platform, channel=channel,
                    release_sequence=release_sequence, upstream_version=version, source_sha=source_sha,
                    issued_at=issued_at, expires_at=expires_at, filename=name,
                    size=source_metadata.st_size, sha256="0" * 64,
                    download_url=f"{origin}{path_prefix}{release_sequence}/{name}")
    manifest_bytes(manifest)  # Check all metadata limits before mutation.
    owned = OwnedPaths()
    try:
        owned.mkdir(output)
        manifest["size"], manifest["sha256"] = copy_payload(source, output / name, owned)
        raw = manifest_bytes(manifest)
        message, public_der = output / ".signing-message", output / ".signing-public.der"
        owned.write(message, DOMAIN + raw)
        owned.write(public_der, SPKI_PREFIX + public)
        signature = openssl_operation(tool, ["pkeyutl", "-sign", "-rawin", "-inkey", str(key), "-in", str(message)])
        if len(signature) != 64:
            raise SigningError("Expected raw 64-byte Ed25519 signature")
        owned.write(output / "manifest.sig", signature)
        openssl_operation(tool, ["pkeyutl", "-verify", "-rawin", "-pubin", "-keyform", "DER",
                                "-inkey", str(public_der), "-in", str(message), "-sigfile", str(output / "manifest.sig")])
        owned.remove(message)
        owned.remove(public_der)
        # Publish the completion file only after checking the detached signature.
        owned.write(output / "manifest.json", raw)
        return manifest
    except Exception:
        owned.cleanup()
        raise


def cli():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    initialize = commands.add_parser("init-key", help="Create a new isolated lab keypair, never overwrite")
    initialize.add_argument("--directory", required=True)
    initialize.add_argument("--openssl", required=True)
    release = commands.add_parser("sign", help="Create a signed lab bundle, not an auto-apply release")
    for option in ("payload", "key-file", "public-key-file", "openssl", "output-dir", "product", "platform",
                   "channel", "version", "source-sha", "origin", "path-prefix"):
        release.add_argument("--" + option, required=True)
    for option in ("release-sequence", "previous-sequence", "issued-at", "expires-at"):
        release.add_argument("--" + option, required=True, type=int)
    args = vars(parser.parse_args())
    command = args.pop("command")
    try:
        if command == "init-key":
            fingerprint = init_key(**args)
            print(f"Lab keypair created; public SHA-256 fingerprint={fingerprint}")
        else:
            sign_release(**args)
            print("Signed lab bundle created; no upload or auto-apply")
    except SigningError as error:
        parser.exit(1, f"Release signing refused: {error}\n")
    except (OSError, ValueError, subprocess.SubprocessError):
        parser.exit(1, "Release signing refused: local operation failed\n")


if __name__ == "__main__":
    cli()
