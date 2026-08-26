#!/usr/bin/env python3
"""Regression tests for the deterministic OnGROW product profile patcher."""

from __future__ import annotations

import base64
import contextlib
import io
import subprocess
import tempfile
import unittest
from pathlib import Path

from apply_ongrow_product_profile import APP_NAME, ProfileError, apply_profile

KEY = base64.b64encode(bytes(range(32))).decode("ascii")
VALID = {
    "role": "customer-desk",
    "app_name": APP_NAME,
    "rendezvous_host": "support.example.test",
    "hbbs_public_key": KEY,
    "control_plane_url": "https://control.example.test",
}
FIXTURE = '''
pub static ref PROD_RENDEZVOUS_SERVER: RwLock<String> = RwLock::new("".to_owned());
pub static ref APP_NAME: RwLock<String> = RwLock::new("RustDesk".to_owned());
pub const RENDEZVOUS_SERVERS: &[&str] = &["rs-ny.rustdesk.com"];
pub const RS_PUB_KEY: &str = "OeVuKk5nlHiXp+APNn0Y3pC1Iwpwn44JGqrQCsWqmBw=";
'''


class ProductProfileTests(unittest.TestCase):
    def make_config(self, source: str = FIXTURE) -> tuple[tempfile.TemporaryDirectory[str], Path]:
        directory = tempfile.TemporaryDirectory()
        path = Path(directory.name) / "config.rs"
        path.write_text(source, encoding="utf-8")
        return directory, path

    def test_valid_profile_replaces_every_trust_boundary(self) -> None:
        directory, path = self.make_config()
        self.addCleanup(directory.cleanup)
        fingerprint = apply_profile(path, **VALID)
        actual = path.read_text(encoding="utf-8")
        self.assertEqual(fingerprint, "630dcd2966c4336691125448bbb25b4ff412a49c732db2c8abc1b8581bd710dd")
        self.assertEqual(actual.count("support.example.test"), 2)
        self.assertIn(APP_NAME, actual)
        self.assertIn(KEY, actual)
        self.assertNotIn("rs-ny.rustdesk.com", actual)
        self.assertNotIn("OeVuKk5nlHiXp+APNn0Y3pC1Iwpwn44JGqrQCsWqmBw=", actual)

    def test_invalid_inputs_fail_without_modifying_source(self) -> None:
        invalid = [
            {"role": "operator"},
            {"app_name": "Other"},
            {"rendezvous_host": "https://support.example.test/path"},
            {"rendezvous_host": "localhost"},
            {"hbbs_public_key": "not-base64"},
            {"hbbs_public_key": base64.b64encode(b"short").decode("ascii")},
            {"control_plane_url": "http://control.example.test"},
            {"control_plane_url": "https://user@control.example.test"},
        ]
        for changed in invalid:
            with self.subTest(changed=changed):
                directory, path = self.make_config()
                try:
                    values = VALID | changed
                    with self.assertRaises(ProfileError):
                        apply_profile(path, **values)
                    self.assertEqual(path.read_text(encoding="utf-8"), FIXTURE)
                finally:
                    directory.cleanup()

    def test_missing_or_ambiguous_anchor_fails_closed(self) -> None:
        for source in (FIXTURE.replace("pub const RS_PUB_KEY", "pub const OLD_RS_PUB_KEY"), FIXTURE + FIXTURE):
            with self.subTest():
                directory, path = self.make_config(source)
                try:
                    with self.assertRaises(ProfileError):
                        apply_profile(path, **VALID)
                finally:
                    directory.cleanup()

    def test_key_is_not_written_to_stdout(self) -> None:
        directory, path = self.make_config()
        self.addCleanup(directory.cleanup)
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            fingerprint = apply_profile(path, **VALID)
            print(fingerprint)
        self.assertNotIn(KEY, output.getvalue())

    def test_cli_requires_every_input(self) -> None:
        script = Path(__file__).with_name("apply_ongrow_product_profile.py")
        result = subprocess.run(
            ["python3", str(script), "--role", "customer-desk"],
            text=True,
            capture_output=True,
            check=False,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("required", result.stderr)


if __name__ == "__main__":
    unittest.main(verbosity=2)
