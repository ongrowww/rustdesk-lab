#!/usr/bin/env python3
"""Run the real Rust isolation policy without the desktop build dependencies."""

import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class CiIsolationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory = tempfile.TemporaryDirectory(prefix="ongrow-ci-isolation-")
        cls.binary = Path(cls.directory.name) / ("isolation.exe" if os.name == "nt" else "isolation")
        subprocess.run([
            "rustc", "--edition=2021", "--test", str(ROOT / "src/ongrow_ci.rs"),
            "-o", str(cls.binary),
        ], check=True)

    @classmethod
    def tearDownClass(cls):
        cls.directory.cleanup()

    def test_runtime_policy(self):
        subprocess.run([str(self.binary)], check=True)

    def test_native_network_boundaries_are_guarded(self):
        rendezvous = (ROOT / "src/rendezvous_mediator.rs").read_text()
        start = rendezvous.split("pub async fn start_all() {", 1)[1]
        self.assertLess(start.index("ongrow_ci::networking_disabled()"), start.index("crate::test_nat_type()"))
        for path, entry in [
            ("src/ongrow_control/mod.rs", "fn control_plane_url()"),
            ("src/ongrow_operator.rs", "fn validated_base_url()"),
        ]:
            body = (ROOT / path).read_text().split(entry, 1)[1]
            self.assertLess(body.index("ongrow_ci::networking_disabled()"), body.index('option_env!("ONGROW_CONTROL_PLANE_URL")'))

    def test_every_mac_smoke_launch_explicitly_requests_isolation(self):
        workflows = list((ROOT / ".github/workflows").glob("ongrow*.yml"))
        checked = 0
        for workflow in workflows:
            text = workflow.read_text()
            if "- name: Smoke test app launch" not in text:
                continue
            step = text.split("- name: Smoke test app launch", 1)[1].split("\n      - name:", 1)[0]
            self.assertIn('ONGROW_CI_SMOKE_TEST: "1"', step, workflow.name)
            checked += 1
        self.assertGreaterEqual(checked, 2)


if __name__ == "__main__":
    unittest.main()
