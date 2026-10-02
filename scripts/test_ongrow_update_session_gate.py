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

    def test_native_suite_and_diagnostics_are_probe_only(self):
        tests = (ROOT / "src/ongrow_update/session_gate_tests.rs").read_text()
        ordinary, native = tests.split("#[cfg(ongrow_session_gate_probe)]\nmod probe {", 1)
        self.assertIn("fn default_policy_has_no_side_effects", ordinary)
        self.assertNotIn("ONGROW_GATE_TEST_ROOT", ordinary)
        self.assertIn('var_os("ONGROW_GATE_TEST_ROOT").expect("isolated test root")', native)
        self.assertIn('module_path!().split_once("::")', native)
        self.assertNotIn('"session_gate::tests::native_child"', native)
        fixture = (FIXTURE / "lib.rs").read_text()
        self.assertIn("#[cfg(not(ongrow_session_gate_probe))]", fixture)
        self.assertIn("compile_error!", fixture)
        windows = (ROOT / "src/ongrow_update/session_gate/windows.rs").read_text()
        self.assertIn("#[cfg(all(test, ongrow_session_gate_probe))]", windows)
        self.assertIn('eprintln!("ONGROW_GATE_REJECT:{category}")', windows)
        self.assertNotIn("{:?}", windows)
        self.assertNotIn("std::env::", windows)
        self.assertIn("#[cfg(all(test, ongrow_session_gate_probe))]\nmod owner_diagnostics {", windows)
        diagnostic = windows.split("mod owner_diagnostics {", 1)[1].split("\nfn inspect(", 1)[0]
        self.assertIn("fn LookupAccountNameLocalW(account: PCWSTR", diagnostic)
        self.assertEqual(diagnostic.count('w!("NT SERVICE\\\\TrustedInstaller")'), 2)
        self.assertNotIn("fn LookupAccountNameW(", diagnostic)
        self.assertNotIn("LookupAccountSid", diagnostic)
        self.assertIn("fn diagnostic_categories_never_grant_service_trust", diagnostic)
        self.assertIn("assert!(!trusted_sid(sid, &user, product, ancestor))", diagnostic)
        self.assertIn('"root-volume"', windows)
        trusted = windows.split("fn trusted_sid(", 1)[1].split("\n}\n", 1)[0]
        self.assertEqual(trusted, '''sid: PSID, user: &User, product: Product, _ancestor: bool) -> bool {
    if sid.0.is_null() { return false; }
    unsafe {
        IsWellKnownSid(sid, WinLocalSystemSid).as_bool()
            || IsWellKnownSid(sid, WinBuiltinAdministratorsSid).as_bool()
            || (product == Product::SupportConsole && EqualSid(sid, user.sid()).is_ok())
    }''')

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
            # Only this private probe gets the native-fixture marker. Do not
            # inherit caller rustflags that could bake product runtime policy.
            environment.pop("CARGO_ENCODED_RUSTFLAGS", None)
            environment["RUSTFLAGS"] = "--check-cfg=cfg(ongrow_session_gate_probe) --cfg ongrow_session_gate_probe"
            environment["ONGROW_GATE_TEST_ROOT"] = str(state)
            environment["CARGO_TARGET_DIR"] = str(scratch / "build")
            command = ["cargo", "test", "--lib", "--manifest-path", str(scratch / "Cargo.toml")]
            if platform == "macos" and os.environ.get("GITHUB_ACTIONS") != "true":
                command.append("--offline")
            try:
                listing = subprocess.run(command + ["--", "--list"], env=environment,
                                         capture_output=True, text=True, timeout=180)
                self.assertEqual(listing.returncode, 0, listing.stderr)
                self.assertIn("session_gate::tests::probe::real_shared_sessions_block_exclusive_until_last_drop: test", listing.stdout)
                subprocess.run(command + ["--", "--test-threads=1"], check=True, env=environment, timeout=180)
                missing_root = environment.copy()
                missing_root.pop("ONGROW_GATE_TEST_ROOT", None)
                missing = subprocess.run(command + ["--", "--exact", "session_gate::tests::probe::real_shared_sessions_block_exclusive_until_last_drop"],
                                         env=missing_root, capture_output=True, text=True, timeout=180)
                self.assertNotEqual(missing.returncode, 0)
                self.assertIn("isolated test root", missing.stdout + missing.stderr)

                # Import the exact source under the full app module path, with
                # no native marker or root. This is not a desktop app build.
                app_source = scratch / "app-shape.rs"
                app_source.write_text(
                    '#![allow(dead_code)]\nextern crate self as hbb_common;\n'
                    '#[cfg(target_os = "macos")] pub extern crate libc;\n'
                    'mod ongrow_update {\n#[path = ' + json.dumps(str(ROOT / "src/ongrow_update/session_gate.rs")) + ']\n'
                    'mod session_gate;\n}\n'
                )
                (scratch / "Cargo.toml").write_text(manifest.rsplit("\n[lib]\n", 1)[0] +
                                                  "\n[lib]\npath = " + json.dumps(str(app_source)) + "\n")
                ordinary = missing_root.copy()
                ordinary["RUSTFLAGS"] = "--check-cfg=cfg(ongrow_session_gate_probe)"
                listing = subprocess.run(command + ["--", "--list"], check=True, env=ordinary,
                                         capture_output=True, text=True, timeout=180)
                self.assertIn("ongrow_update::session_gate::tests::default_policy_has_no_side_effects: test", listing.stdout)
                self.assertNotIn("::probe::", listing.stdout)
                self.assertNotIn("native_child", listing.stdout)
                subprocess.run(command + ["--", "--test-threads=1"], check=True, env=ordinary, timeout=180)
            finally:
                if state_directory is not None:
                    state_directory.cleanup()


if __name__ == "__main__":
    unittest.main()
