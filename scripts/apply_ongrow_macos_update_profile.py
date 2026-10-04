#!/usr/bin/env python3
"""Embed public macOS update metadata. Installation remains locked."""
import argparse
import base64
import os
import plistlib
import re
import stat
import tempfile
from pathlib import Path

PRODUCTS = {
    "customer-desk": ("de.ongrow.supportdesk", "OnGROW Support Desk"),
    "support-console": ("de.ongrow.supportconsole", "OnGROW Support Console"),
}


def apply(root, role, feed, public_key, sequence):
    # macOS /tmp and /var are system symlinks. Canonicalize the explicitly
    # selected root, then reject symlinks inside the metadata input tree.
    root = root.resolve(strict=True)
    if role not in PRODUCTS:
        raise ValueError("unknown product")
    if feed != f"https://ongrow.de/assets/updates/{role}/macos-arm64/appcast.xml":
        raise ValueError("feed outside product allowlist")
    try:
        key = base64.b64decode(public_key, validate=True)
    except ValueError as error:
        raise ValueError("invalid public key") from error
    if len(key) != 32 or base64.b64encode(key).decode() != public_key:
        raise ValueError("invalid public key")
    if not re.fullmatch(r"[1-9][0-9]{0,7}", str(sequence)) or int(sequence) > 99_989_999:
        raise ValueError("invalid release sequence")
    plist_path = root / "flutter/macos/Runner/Info.plist"
    config_path = root / "flutter/macos/Runner/Configs/AppInfo.xcconfig"
    for path in (plist_path, config_path):
        metadata = path.lstat()
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1:
            raise ValueError("profile input must be an independent regular file")
        for parent in path.parents:
            if parent.is_symlink():
                raise ValueError("symlink profile path rejected")
    original_stat = plist_path.stat()
    raw = plist_path.read_bytes()
    config = config_path.read_text()
    bundle_id, name = PRODUCTS[role]
    for field, value in (("PRODUCT_NAME", name), ("PRODUCT_BUNDLE_IDENTIFIER", bundle_id)):
        if len(re.findall(rf"^{field} = {re.escape(value)}$", config, re.M)) != 1:
            raise ValueError("product metadata mismatch")
    # plistlib alone accepts duplicate keys. Reject them before parsing.
    keys = re.findall(rb"<key>([^<]+)</key>", raw)
    if len(keys) != len(set(keys)):
        raise ValueError("duplicate plist key")
    info = plistlib.loads(raw)
    if info.get("CFBundleIdentifier") != "$(PRODUCT_BUNDLE_IDENTIFIER)" or info.get("CFBundleDisplayName") != name:
        raise ValueError("product metadata mismatch")
    if info.get("CFBundleVersion") != "$(FLUTTER_BUILD_NUMBER)" or info.get("OnGrowUpdateApplyEnabled") is not False:
        raise ValueError("missing pristine update anchors")
    if any(key.startswith("SU") for key in info):
        raise ValueError("update profile already present")
    counter = int(sequence)
    bundle_version = f"{counter // 10000 + 1}.{counter // 100 % 100}.{counter % 100}"
    info.update({"OnGrowUpdateRole": role, "OnGrowUpdateSequence": str(sequence), "CFBundleVersion": bundle_version,
                 "SUFeedURL": feed, "SUPublicEDKey": public_key,
                 "SUVerifyUpdateBeforeExtraction": True, "SURequireSignedFeed": True,
                 "SUSignedFeedFailureExpirationInterval": 0,
                 "SUEnableSystemProfiling": False, "SUEnableAutomaticChecks": True,
                 "SUAutomaticallyUpdate": True, "OnGrowUpdateApplyEnabled": False})
    result = plistlib.dumps(info, sort_keys=False)
    # All validation has completed; no source/config writes before this point.
    descriptor, temporary = tempfile.mkstemp(prefix=".ongrow-update-profile-", dir=plist_path.parent)
    temporary = Path(temporary)
    created_stat = os.fstat(descriptor)
    try:
        with os.fdopen(descriptor, "wb") as stream:
            os.fchmod(stream.fileno(), stat.S_IMODE(original_stat.st_mode))
            stream.write(result)
            stream.flush()
            os.fsync(stream.fileno())
        current = plist_path.lstat()
        if (current.st_dev, current.st_ino, current.st_mtime_ns, current.st_size, current.st_nlink) != (
                original_stat.st_dev, original_stat.st_ino, original_stat.st_mtime_ns, original_stat.st_size, 1):
            raise ValueError("metadata changed during profile preparation")
        os.replace(temporary, plist_path)
    finally:
        if temporary.exists():
            current = temporary.lstat()
            if (current.st_dev, current.st_ino) == (created_stat.st_dev, created_stat.st_ino):
                temporary.unlink()
            else:
                raise ValueError("temporary profile ownership changed; file left untouched")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--role", required=True, choices=PRODUCTS)
    parser.add_argument("--feed-url", required=True)
    parser.add_argument("--public-key", required=True)
    parser.add_argument("--sequence", required=True)
    args = parser.parse_args()
    try:
        apply(args.root, args.role, args.feed_url, args.public_key, args.sequence)
    except (ValueError, OSError, plistlib.InvalidFileException) as error:
        parser.exit(1, f"macOS update profile rejected: {error}\n")


if __name__ == "__main__":
    main()
