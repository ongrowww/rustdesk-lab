#!/usr/bin/env python3
"""Apply the fail-closed OnGROW customer product profile to hbb_common."""

from __future__ import annotations

import argparse
import base64
import binascii
import hashlib
import ipaddress
import re
from pathlib import Path
from urllib.parse import urlsplit

ROLE = "customer-desk"
APP_NAME = "OnGROW Support Desk"
CONFIG_PATH = Path("libs/hbb_common/src/config.rs")


class ProfileError(ValueError):
    """Raised when a profile input or source anchor is invalid."""


def _validate_hostname(value: str) -> str:
    if not value or value != value.strip() or "://" in value or any(c in value for c in "/?#@:"):
        raise ProfileError("rendezvous host must be a hostname without scheme, port, or path")
    try:
        ipaddress.ip_address(value)
        return value
    except ValueError:
        pass
    if len(value) > 253:
        raise ProfileError("rendezvous hostname is too long")
    labels = value.rstrip(".").split(".")
    if len(labels) < 2 or any(
        not re.fullmatch(r"[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?", label)
        for label in labels
    ):
        raise ProfileError("rendezvous host is not a valid DNS hostname")
    return value.rstrip(".").lower()


def _decode_public_key(value: str) -> bytes:
    try:
        decoded = base64.b64decode(value, validate=True)
    except (binascii.Error, ValueError) as exc:
        raise ProfileError("hbbs public key must be valid base64") from exc
    if len(decoded) != 32:
        raise ProfileError("hbbs public key must decode to exactly 32 bytes")
    return decoded


def _validate_control_plane_url(value: str) -> str:
    parsed = urlsplit(value)
    if (
        parsed.scheme != "https"
        or not parsed.hostname
        or parsed.username is not None
        or parsed.password is not None
        or parsed.query
        or parsed.fragment
        or not parsed.netloc
    ):
        raise ProfileError("control-plane base URL must be an absolute HTTPS URL")
    return value.rstrip("/")


def _replace_unique(source: str, old: str, new: str, description: str) -> str:
    count = source.count(old)
    if count != 1:
        raise ProfileError(f"expected exactly one {description} anchor, found {count}")
    return source.replace(old, new, 1)


def apply_profile(
    config_path: Path,
    *,
    role: str,
    app_name: str,
    rendezvous_host: str,
    hbbs_public_key: str,
    control_plane_url: str,
) -> str:
    if role != ROLE:
        raise ProfileError(f"role must be {ROLE}")
    if app_name != APP_NAME:
        raise ProfileError(f"app name must be {APP_NAME}")
    host = _validate_hostname(rendezvous_host)
    key_bytes = _decode_public_key(hbbs_public_key)
    _validate_control_plane_url(control_plane_url)

    source = config_path.read_text(encoding="utf-8")
    source = _replace_unique(
        source,
        'pub static ref PROD_RENDEZVOUS_SERVER: RwLock<String> = RwLock::new("".to_owned());',
        f'pub static ref PROD_RENDEZVOUS_SERVER: RwLock<String> = RwLock::new("{host}".to_owned());',
        "production rendezvous",
    )
    source = _replace_unique(
        source,
        'pub static ref APP_NAME: RwLock<String> = RwLock::new("RustDesk".to_owned());',
        f'pub static ref APP_NAME: RwLock<String> = RwLock::new("{APP_NAME}".to_owned());',
        "application name",
    )
    source = _replace_unique(
        source,
        'pub const RENDEZVOUS_SERVERS: &[&str] = &["rs-ny.rustdesk.com"];',
        f'pub const RENDEZVOUS_SERVERS: &[&str] = &["{host}"];',
        "fallback rendezvous",
    )
    source = _replace_unique(
        source,
        'pub const RS_PUB_KEY: &str = "OeVuKk5nlHiXp+APNn0Y3pC1Iwpwn44JGqrQCsWqmBw=";',
        f'pub const RS_PUB_KEY: &str = "{hbbs_public_key}";',
        "hbbs trust anchor",
    )
    config_path.write_text(source, encoding="utf-8")
    return hashlib.sha256(key_bytes).hexdigest()


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--role", required=True)
    parser.add_argument("--app-name", required=True)
    parser.add_argument("--rendezvous-host", required=True)
    parser.add_argument("--hbbs-public-key", required=True)
    parser.add_argument("--control-plane-url", required=True)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        fingerprint = apply_profile(
            CONFIG_PATH,
            role=args.role,
            app_name=args.app_name,
            rendezvous_host=args.rendezvous_host,
            hbbs_public_key=args.hbbs_public_key,
            control_plane_url=args.control_plane_url,
        )
    except (OSError, ProfileError) as exc:
        raise SystemExit(f"product profile rejected: {exc}") from exc
    print(f"OnGROW customer profile applied; hbbs key sha256={fingerprint}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
