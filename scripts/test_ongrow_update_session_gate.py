#!/usr/bin/env python3
"""Compile the actual gate alone, then run source-boundary and native OS tests."""
import os
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "scripts/fixtures/ongrow_session_gate_probe"


class SessionGateTests(unittest.TestCase):
    def test_baked_default_off_no_runtime_override(self):
        source = (ROOT / "src/ongrow_update/session_gate.rs").read_text()
        self.assertIn('option_env!("ONGROW_UPDATE_SESSION_GATE")', source)
        self.assertIn("None => return Ok(None)", source)
        self.assertNotIn("std::env::", source)
        self.assertNotIn("Config::", source)
        self.assertIn("struct VerifiedHealth(())", source)
        self.assertNotIn("fn new", source)

    def test_admission_precedes_effects_and_both_outgoing_callers_hold_lease(self):
        server = (ROOT / "src/server.rs").read_text().split("pub async fn create_tcp_connection(", 1)[1]
        self.assertLess(server.index("session_gate::admit()?"), server.index("Config::get_key_pair()"))
        self.assertIn("meta, session_lease).await", server)
        incoming = (ROOT / "src/server/connection.rs").read_text().split("pub async fn start(", 1)[1]
        self.assertLess(incoming.index("let _session_lease = session_lease"), incoming.index("ConnectionID::new"))
        ui = (ROOT / "src/ui_session_interface.rs").read_text().split("pub async fn io_loop<T:", 1)[1]
        self.assertLess(ui.index("session_gate::admit()"), ui.index("crate::get_key(false).await"))
        self.assertLess(ui.index("session_gate::admit()"), ui.index("handler.is_port_forward()"))
        forward_ui = ui.split("if handler.is_port_forward() {", 1)[1]
        self.assertLess(forward_ui.index("drop(_early_session_lease)"), forward_ui.index("start_one_port_forward("))
        client = (ROOT / "src/client.rs").read_text().split("pub async fn start(", 1)[1]
        self.assertLess(client.index("session_gate::admit()?"), client.index("Self::_start("))
        self.assertIn("Ok((x.0, x.1, session_lease))", client)
        remote = (ROOT / "src/client/io_loop.rs").read_text()
        self.assertIn("let _session_lease = session_lease;", remote)
        forwarding = (ROOT / "src/port_forward.rs").read_text()
        self.assertIn("run_forward(forward, stream, session_lease).await", forwarding)
        self.assertIn("Ok(Some((stream, session_lease)))", forwarding)
        self.assertIn("let _session_lease = session_lease;", forwarding)
        self.assertEqual(sum(path.read_text().count("Client::start(") for path in [ROOT / "src/client/io_loop.rs", ROOT / "src/port_forward.rs"]), 2)

    def test_os_state_is_never_automatically_reset_or_replaced(self):
        for platform in ["macos", "windows"]:
            source = (ROOT / f"src/ongrow_update/session_gate/{platform}.rs").read_text()
            self.assertIn("admission-v1.lock", source)
            self.assertIn("state-v1.journal", source)
            self.assertNotIn("std::env::", source)
            for forbidden in ["remove_file", "rename(", "set_len(", "process::id", "sleep("]:
                self.assertNotIn(forbidden, source)
            self.assertIn("MissingGate", source)
            self.assertIn("self.journal.sync_all()", source)
        self.assertIn("LOCK_NB", (ROOT / "src/ongrow_update/session_gate/macos.rs").read_text())
        self.assertIn("LOCKFILE_FAIL_IMMEDIATELY", (ROOT / "src/ongrow_update/session_gate/windows.rs").read_text())

    @unittest.skipUnless(sys.platform in ["darwin", "win32"], "native gate requires macOS or Windows")
    def test_real_native_gate(self):
        platform = "windows" if os.name == "nt" else "macos"
        if platform == "windows" and os.environ.get("GITHUB_ACTIONS") != "true":
            self.fail("Windows probe runs only on its disposable GitHub Runner")
        target = ROOT / "target/ongrow-session-gate-tests"
        target.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix="probe-", dir=target) as directory:
            scratch = Path(directory)
            manifest = (FIXTURE / f"{platform}.toml").read_text()
            # Absolute original source paths keep this a test of the production
            # module, not a generated replacement implementation.
            manifest += "\n[lib]\npath = " + json.dumps(str(FIXTURE / "lib.rs")) + "\n"
            (scratch / "Cargo.toml").write_text(manifest)
            if platform == "windows":
                # Preserve the application's pinned Windows transitive versions
                # in an isolated scratch lock, never rewriting Root Cargo.lock.
                shutil.copyfile(ROOT / "Cargo.lock", scratch / "Cargo.lock")
                state_directory = tempfile.TemporaryDirectory(prefix="ongrow-gate-state-")
                state = Path(state_directory.name)
            else:
                state_directory = None
                state = scratch / "state"
                state.mkdir(mode=0o700)
            environment = os.environ.copy()
            environment.pop("ONGROW_UPDATE_SESSION_GATE", None)
            environment.pop("ONGROW_PRODUCT_ROLE", None)
            environment["ONGROW_GATE_TEST_ROOT"] = str(state)
            environment["CARGO_TARGET_DIR"] = str(scratch / "build")
            command = ["cargo", "test", "--manifest-path", str(scratch / "Cargo.toml")]
            if platform == "macos" and os.environ.get("GITHUB_ACTIONS") != "true":
                command.append("--offline")
            try:
                subprocess.run(command + ["--", "--test-threads=1"], check=True, env=environment, timeout=180)
            finally:
                if state_directory is not None:
                    state_directory.cleanup()


if __name__ == "__main__":
    unittest.main()
