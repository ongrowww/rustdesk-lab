#!/usr/bin/env python3
"""Probe the original gate's exclusive Pending lease, without installer authority."""
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import unittest

from test_ongrow_update_store import addition, ORIGINAL
from test_ongrow_update_session_gate import windows_probe_state_directory

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "scripts/fixtures/ongrow_pending_recovery_probe/lib.rs"
OLD_ADDITIONS = {
    "session_gate.rs": (2534, "8d183bf159bb94d1198233a61f9f5df4992dfaa07adc312d8e3c05fa3fb287d6"),
    "session_gate/macos.rs": (3979, "6b39337d3b1270ce96b96e20b771488ade5e0ee8f727a3ff20297d7d6583fb72"),
    "session_gate/windows.rs": (3852, "a83038ad7be4485525e2ad631db7b14a221189d54cbfb2f2697184e50884fe2a"),
}
CHECK_CFG = " ".join("--check-cfg=cfg(" + marker + ")" for marker in (
    "ongrow_session_gate_probe", "ongrow_update_runtime_probe", "ongrow_update_store_probe",
    "ongrow_pending_recovery_probe"))


def source(path):
    return path.read_text(encoding="utf-8", errors="strict")


class SourceTests(unittest.TestCase):
    def test_original_bodies_and_existing_addition_bytes_are_unchanged(self):
        for name, (length, digest) in OLD_ADDITIONS.items():
            with self.subTest(source=name):
                block = addition(ROOT / "src/ongrow_update" / name, ORIGINAL[name]).encode("utf-8")
                self.assertEqual(hashlib.sha256(block[:length]).hexdigest(), digest)
                self.assertGreater(len(block), length)

    def test_factory_owns_same_exclusive_lock_without_new_authority(self):
        blocks = {name: addition(ROOT / "src/ongrow_update" / name, ORIGINAL[name])
                  .encode("utf-8")[length:].decode("utf-8", errors="strict")
                  for name, (length, _) in OLD_ADDITIONS.items()}
        gate = blocks["session_gate.rs"]
        self.assertIn("pub(crate) struct PendingRecoveryLease { _lock: os::Lock }", gate)
        self.assertIn("lock.require_pending()?;\n    Ok(PendingRecoveryLease { _lock: lock })", gate)
        self.assertIn("let product = startup_policy()?.ok_or(Error::Disabled)?;\n    pending_recovery_lease(os::exclusive(product)?)", gate)
        self.assertEqual(gate.count('#[cfg(any(target_os = "macos", target_os = "windows"))]'), 3)
        self.assertIn("#[cfg(all(test, ongrow_pending_recovery_probe, any(target_os", gate)
        for block in blocks.values():
            for forbidden in ("Clone", "pub fn", "mark_pending", "mark_ready", "VerifiedHealth",
                              "Transaction", "std::env", "sync_all", "remove_file", "initialize(",
                              "unwrap(", "expect(", "fn commit", "fn path"):
                self.assertNotIn(forbidden, block)
        for name in ("session_gate/macos.rs", "session_gate/windows.rs"):
            self.assertEqual(blocks[name], '''impl Lock {
    pub(super) fn require_pending(&mut self) -> Result<(), Error> {
        if !self.exclusive { return Err(Error::Untrusted); }
        if self.state()? != PENDING { return Err(Error::Untrusted); }
        Ok(())
    }
}
''')

    def test_probe_uses_original_module_and_bounds_own_children(self):
        fixture = source(FIXTURE)
        self.assertIn('#[cfg(not(ongrow_pending_recovery_probe))]', fixture)
        self.assertIn('#[path = "../../../src/ongrow_update/session_gate.rs"]', fixture)
        native = source(ROOT / "src/ongrow_update/pending_recovery_tests.rs")
        self.assertGreaterEqual(len(re.findall(r"#\[test\]\s*fn (?!native_child)\w+", native)), 8)
        for required in ("pending_recovery_lease(os::fixture_acquire(path, true)?)",
                         "recv_timeout(Duration::from_secs(10))", "impl Drop for ProbeChild",
                         "self.child.kill()", "self.child.wait()", 'fixture.child("pending")',
                         'fixture.child("recovery")', "error(pending_recovery_lease(shared), Error::Untrusted)"):
            self.assertIn(required, native)
        self.assertIn('line.ends_with("PENDING_RECOVERY_CHILD_READY")', native)
        for forbidden in ("VerifiedHealth", "mark_ready", "commit_healthy", "thread::sleep", "killall"):
            self.assertNotIn(forbidden, native)

    def test_workflow_trust_freeze_and_only_isolated_probes(self):
        workflow = source(ROOT / ".github/workflows/ongrow-pending-recovery-lab.yml")
        for required in ("contents: read", "os: [macos-14, windows-2022]", "toolchain: '1.81.0'",
                         "github.event.pull_request.head.repo.full_name == github.repository",
                         "persist-credentials: false", "fetch-depth: 0",
                         "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1",
                         "dtolnay/rust-toolchain@e97e2d8cc328f1b50210efc529dca0028893a2d9",
                         "SOURCE_SHA: ${{ github.event.pull_request.head.sha || github.sha }}",
                         'test "$(git rev-parse HEAD)" = "$SOURCE_SHA"',
                         'git diff --exit-code d8b2202e3e8747f949d0a94625aa02c8bfe18ce7 "$SOURCE_SHA" -- .',
                         "python scripts/test_ongrow_pending_recovery.py", "python scripts/test_ongrow_update_store.py",
                         "python scripts/test_ongrow_update_session_gate.py", "python scripts/test_ongrow_product_profile.py",
                         "python scripts/test_ongrow_ci_isolation.py", "      - '.gitattributes'"):
            self.assertIn(required, workflow)
        self.assertEqual(re.findall(r"'\:\(exclude\)([^']+)'", workflow), [
            "src/ongrow_update/session_gate.rs", "src/ongrow_update/session_gate/macos.rs",
            "src/ongrow_update/session_gate/windows.rs", "src/ongrow_update/pending_recovery_tests.rs",
            "scripts/test_ongrow_pending_recovery.py", "scripts/fixtures/ongrow_pending_recovery_probe/lib.rs",
            ".github/workflows/ongrow-pending-recovery-lab.yml", "docs/ongrow-pending-recovery.md"])
        for forbidden in ("secrets.", "upload-artifact", "sudo", "flutter", "Set-Acl", "cargo build"):
            self.assertNotIn(forbidden, workflow)

    def test_gate_snapshot_checks_zero_length_identity_and_actual_lock_denial(self):
        native = source(ROOT / "src/ongrow_update/pending_recovery_tests.rs")
        snapshot = native.split("    fn snapshot(&self)", 1)[1].split("    fn child(&self", 1)[0]
        self.assertIn('assert_eq!(fs::metadata(&path).unwrap().len(), 0, "snapshot requires an empty gate");', snapshot)
        self.assertIn('''#[cfg(target_os = "windows")]
                { Vec::new() }
                #[cfg(target_os = "macos")]
                { fs::read(&path).unwrap() }
            } else {
                fs::read(&path).unwrap()
            };
            (bytes, identity(&path))''', snapshot)
        case = native.split("fn gate_snapshot_preserves_lock_and_rejects_nonempty_gate()", 1)[1].split("#[test]", 1)[0]
        for required in ('let lease = recover(&fixture.0).unwrap();',
                         'let read_error = fs::read(&gate).unwrap_err();',
                         'assert_eq!(read_error.raw_os_error(), Some(33));',
                         '#[cfg(target_os = "macos")]',
                         'fs::write(&gate, [1]).unwrap();',
                         'assert_eq!(fs::metadata(&gate).unwrap().len(), 1);',
                         'assert!(std::panic::catch_unwind(|| fixture.snapshot()).is_err());',
                         'println!("WINDOWS_GATE_SNAPSHOT_PASS");'):
            self.assertIn(required, case)
        self.assertEqual(case.count('assert_eq!(fs::read(&gate).unwrap(), Vec::<u8>::new());'), 3)
        self.assertEqual(case.count('assert_eq!(before, fixture.snapshot());'), 2)
        self.assertLess(case.index('assert_eq!(before, fixture.snapshot());'), case.index('drop(lease);'))
        self.assertLess(case.index('drop(lease);'), case.index('fs::write(&gate, [1]).unwrap();'))
        self.assertIn('GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut information)', native)
        self.assertNotIn("unwrap_or_default", snapshot)


class NativeTests(unittest.TestCase):
    def test_actual_native_cases_crashes_missing_input_and_ordinary_app(self):
        self.assertIn(sys.platform, ("darwin", "win32"), "Native host required; no silent skip")
        platform = "windows" if os.name == "nt" else "macos"
        # Validate the Windows OS parent before any build or state side effect.
        state_directory = windows_probe_state_directory() if platform == "windows" else None
        if state_directory is not None:
            self.addCleanup(state_directory.cleanup)
        target = ROOT / "target/ongrow-pending-recovery-probe"
        target.mkdir(parents=True, exist_ok=True)
        environment = os.environ.copy()
        for name in ("ONGROW_UPDATE_SESSION_GATE", "ONGROW_PRODUCT_ROLE", "CARGO_ENCODED_RUSTFLAGS",
                     "ONGROW_PENDING_RECOVERY_CHILD_ROOT", "ONGROW_PENDING_RECOVERY_CHILD_ACTION"):
            environment.pop(name, None)
        environment["RUSTFLAGS"] = CHECK_CFG + " --cfg ongrow_pending_recovery_probe"
        version = subprocess.run(["rustc", "--version"], env=environment, capture_output=True,
                                 encoding="utf-8", errors="strict", timeout=10, check=True)
        self.assertTrue(version.stdout.startswith("rustc 1.81.0 "))
        with tempfile.TemporaryDirectory(prefix="build-", dir=target) as directory:
            scratch = Path(directory)
            if state_directory is None:
                state = scratch / "state"
                state.mkdir(mode=0o700)
            else:
                state = Path(state_directory.name)
            environment["ONGROW_PENDING_RECOVERY_TEST_ROOT"] = str(state.resolve())
            environment["CARGO_TARGET_DIR"] = str(target / "build")
            original = source(ROOT / f"scripts/fixtures/ongrow_session_gate_probe/{platform}.toml")
            manifest = original + "\n[lib]\npath = " + json.dumps(str(FIXTURE)) + "\n"
            (scratch / "Cargo.toml").write_text(manifest, encoding="utf-8", errors="strict")
            shutil.copyfile(ROOT / "Cargo.lock", scratch / "Cargo.lock")
            command = ["cargo", "test", "--lib", "--manifest-path", str(scratch / "Cargo.toml")]
            if environment.get("GITHUB_ACTIONS") != "true":
                command.append("--offline")

            def run(arguments, env=environment, timeout=180):
                return subprocess.run(command + arguments, env=env, capture_output=True,
                                      encoding="utf-8", errors="strict", timeout=timeout)

            listing = run(["--", "--list"])
            self.assertEqual(listing.returncode, 0, listing.stderr)
            names = re.findall(r"#\[test\]\s*fn (\w+)", source(ROOT / "src/ongrow_update/pending_recovery_tests.rs"))
            actual = [name for name in names if name != "native_child"]
            self.assertGreaterEqual(len(actual), 10)
            self.assertIn("gate_snapshot_preserves_lock_and_rejects_nonempty_gate", actual)
            prefix = "session_gate::pending_recovery_tests::"
            for name in names:
                self.assertIn(prefix + name + ": test", listing.stdout)
            serial = environment.copy()
            serial["RUST_TEST_THREADS"] = "1"
            for label, native_environment in (("inherited environment", environment),
                                              ("RUST_TEST_THREADS=1", serial)):
                with self.subTest(native_environment=label):
                    result = run([prefix, "--", "--test-threads=1", "--nocapture"], env=native_environment)
                    self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                    self.assertIn(f"{len(names)} passed; 0 failed; 0 ignored", result.stdout)
                    self.assertIn("NATIVE_PENDING_RECOVERY_PASS", result.stdout)
                    if platform == "windows":
                        self.assertIn("WINDOWS_GATE_SNAPSHOT_PASS", result.stdout)
                    print(label + "\n" + result.stdout)
            missing = environment.copy()
            missing.pop("ONGROW_PENDING_RECOVERY_TEST_ROOT", None)
            result = run(["--", "--exact", prefix + actual[0]], env=missing, timeout=30)
            self.assertNotEqual(result.returncode, 0, "Missing input must fail, not skip")
            self.assertIn("isolated pending recovery test root", result.stdout + result.stderr)
            print("MISSING_PENDING_RECOVERY_INPUT_REJECTED")
            app = ('#![allow(dead_code)]\nextern crate self as hbb_common;\n'
                   '#[cfg(target_os = "macos")] pub extern crate libc;\n'
                   'mod ongrow_update {\n#[path = ' + json.dumps(str(ROOT / "src/ongrow_update/session_gate.rs")) +
                   ']\nmod session_gate;\n}\n')
            (scratch / "app-shape.rs").write_text(app, encoding="utf-8", errors="strict")
            (scratch / "Cargo.toml").write_text(original + '\n[lib]\npath = "app-shape.rs"\n',
                                              encoding="utf-8", errors="strict")
            ordinary = missing.copy()
            ordinary["RUSTFLAGS"] = CHECK_CFG
            listing = run(["--", "--list"], env=ordinary)
            self.assertEqual(listing.returncode, 0, listing.stderr)
            self.assertNotIn("pending_recovery_tests::", listing.stdout)
            self.assertNotIn("::probe::", listing.stdout)
            self.assertIn("ongrow_update::session_gate::tests::default_policy_has_no_side_effects: test", listing.stdout)
            result = run(["--", "--test-threads=1"], env=ordinary)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertIn("1 passed; 0 failed; 0 ignored", result.stdout)
            print("ORDINARY_APP_NO_NATIVE_RECOVERY_CASES: PASS")


if __name__ == "__main__":
    unittest.main()
