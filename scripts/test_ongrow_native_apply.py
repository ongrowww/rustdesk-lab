#!/usr/bin/env python3
"""Original native gate tests and a CI-only adapter, without production authority."""
import argparse
import gc
import json
import os
from pathlib import Path
import queue
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import unittest
from unittest import mock

from test_ongrow_update_session_gate import windows_probe_state_directory

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "scripts/fixtures/ongrow_native_apply_probe/lib.rs"
PREFIX = "session_gate::native_apply_tests::"
CHECK_CFG = " ".join("--check-cfg=cfg(" + name + ")" for name in (
    "ongrow_session_gate_probe", "ongrow_update_runtime_probe", "ongrow_update_store_probe",
    "ongrow_pending_recovery_probe", "ongrow_native_apply_probe"))


def text(path):
    return path.read_text(encoding="utf-8", errors="strict")


def clean_environment():
    environment = os.environ.copy()
    for name in list(environment):
        if name.startswith("ONGROW_") or name == "CARGO_ENCODED_RUSTFLAGS":
            environment.pop(name)
    return environment


def require_ci(platform):
    expected = {"darwin": "macOS", "win32": "Windows"}
    if (platform not in expected or sys.platform != platform or
            os.environ.get("GITHUB_ACTIONS") != "true" or
            os.environ.get("RUNNER_OS") != expected[platform]):
        raise RuntimeError("explicit disposable native CI runner required")


class FixtureDirectory:
    """Owned disposable fixture with retention that also disables its finalizer."""
    def __init__(self, **arguments):
        self.temporary = tempfile.TemporaryDirectory(**arguments)
        self.name = self.temporary.name
        self.retained = False
        self.native_in_flight = False

    def retain(self):
        self.retained = True
        self.temporary._finalizer.detach()

    def cleanup(self):
        if self.native_in_flight:
            self.retain()
        if not self.retained:
            self.temporary.cleanup()

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.cleanup()


class ProbeBuild:
    def __enter__(self):
        self.state_directory = None
        self.scratch_directory = None
        self.native_fixtures = []
        try:
            return self._enter()
        except Exception:
            self.__exit__(None, None, None)
            raise

    def _enter(self):
        if sys.platform not in ("darwin", "win32"):
            raise RuntimeError("native host required, never skip")
        self.state_directory = None
        # Windows validates the real KnownFolder and every ancestor before writing.
        if sys.platform == "win32":
            self.state_directory = windows_probe_state_directory(create=FixtureDirectory)
        target = ROOT / "target/ongrow-native-apply-probe"
        target.mkdir(parents=True, exist_ok=True)
        self.scratch_directory = FixtureDirectory(prefix="build-", dir=target)
        self.scratch = Path(self.scratch_directory.name)
        self.state = Path(self.state_directory.name) if self.state_directory else self.scratch / "state"
        if not self.state_directory:
            self.state.mkdir(mode=0o700)
        self.environment = clean_environment()
        self.environment.update(RUSTFLAGS=CHECK_CFG + " --cfg ongrow_native_apply_probe",
                                CARGO_TARGET_DIR=str(target / "build"),
                                ONGROW_NATIVE_APPLY_TEST_ROOT=str(self.state.resolve()))
        version = subprocess.run(["rustc", "--version"], env=self.environment, capture_output=True,
                                 encoding="utf-8", errors="strict", timeout=10, check=True)
        if not version.stdout.startswith("rustc 1.81.0 "):
            raise RuntimeError("exact cached Rust 1.81.0 required")
        platform = "windows" if sys.platform == "win32" else "macos"
        self.original = text(ROOT / f"scripts/fixtures/ongrow_session_gate_probe/{platform}.toml")
        manifest = self.original + "\n[lib]\npath = " + json.dumps(str(FIXTURE)) + "\n"
        (self.scratch / "Cargo.toml").write_text(manifest, encoding="utf-8", errors="strict")
        shutil.copyfile(ROOT / "Cargo.lock", self.scratch / "Cargo.lock")
        self.command = ["cargo", "test", "--lib", "--manifest-path", str(self.scratch / "Cargo.toml")]
        if os.environ.get("GITHUB_ACTIONS") != "true":
            self.command.append("--offline")
        result = self.run(["--no-run", "--message-format=json"])
        if result.returncode:
            raise RuntimeError(result.stdout + result.stderr)
        executables = [item["executable"] for line in result.stdout.splitlines()
                       if (item := json.loads(line)).get("reason") == "compiler-artifact"
                       and item.get("executable")]
        if len(executables) != 1:
            raise RuntimeError("one isolated gate test executable required")
        self.binary = Path(executables[0])
        return self

    def run(self, arguments, environment=None, timeout=180):
        return subprocess.run(self.command + arguments, env=environment or self.environment,
                              capture_output=True, encoding="utf-8", errors="strict", timeout=timeout)

    def __exit__(self, *_):
        if any(fixture.native_in_flight or fixture.retained for fixture in self.native_fixtures):
            self.retain_unknown()
        if self.scratch_directory:
            self.scratch_directory.cleanup()
        if self.state_directory:
            self.state_directory.cleanup()

    def track_native_fixture(self, fixture):
        self.native_fixtures.append(fixture)

    def retain_unknown(self):
        for directory in (self.scratch_directory, self.state_directory, *self.native_fixtures):
            if directory:
                directory.retain()


class ProbeChild:
    def __init__(self, build, root, action):
        if action not in ("bootstrap", "session", "transaction", "recovery", "admit"):
            raise RuntimeError("explicit isolated probe action required")
        environment = build.environment.copy()
        environment.update(ONGROW_NATIVE_APPLY_CHILD_ROOT=str(root),
                           ONGROW_NATIVE_APPLY_CHILD_ACTION=action)
        self.process = subprocess.Popen(
            [str(build.binary), "--exact", PREFIX + "native_child", "--nocapture"],
            env=environment, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            encoding="utf-8", errors="strict")
        self.messages = queue.Queue(maxsize=1)
        self.output = []
        def read():
            try:
                size = 0
                for line in iter(lambda: self.process.stdout.readline(4097), ""):
                    size += len(line.encode("utf-8"))
                    if len(line) > 4096 or size > 8192:
                        raise RuntimeError("bounded child output exceeded")
                    self.output.append(line)
                    if "NATIVE_APPLY_STATUS=" in line:
                        self.messages.put_nowait(line.split("NATIVE_APPLY_STATUS=", 1)[1].strip())
            except Exception as error:
                try:
                    self.messages.put_nowait(error)
                except queue.Full:
                    pass
        self.reader = threading.Thread(target=read, daemon=True)
        self.reader.start()
        try:
            self.status = self.messages.get(timeout=10)
            if isinstance(self.status, Exception):
                raise self.status
            if self.status not in ("Ready", "Pending", "Busy", "MissingGate", "Untrusted", "Io", "Disabled"):
                raise RuntimeError("unexpected actual gate status")
            self.assert_alive()
        except Exception:
            self.close(crash=True)
            raise

    def assert_alive(self):
        if self.process.poll() is not None:
            raise RuntimeError("gate child lost before explicit end")

    def close(self, crash=False):
        try:
            if crash:
                self.assert_alive()
                self.process.kill()
            else:
                self.assert_alive()
                self.process.stdin.write("exit\n")
                self.process.stdin.flush()
            code = self.process.wait(timeout=10)
            if (crash and code == 0) or (not crash and code != 0):
                raise RuntimeError("unexpected gate child exit")
        finally:
            if self.process.poll() is None:
                self.process.kill()
                self.process.wait(timeout=10)
            self.reader.join(timeout=10)
            self.process.stdin.close()
            self.process.stdout.close()
            if self.reader.is_alive():
                raise RuntimeError("bounded gate reader exit failed")


class NativeGate:
    """Own fixture root only. No arbitrary installer argv or production root interface."""
    def __init__(self, build):
        self.build = build
        self.directory = FixtureDirectory(prefix="guardian-", dir=build.state)
        self.root = Path(self.directory.name)
        self.owner = None
        self.mode = None
        self.native_in_flight = False
        try:
            if self.request("bootstrap") != "Ready":
                raise RuntimeError("fresh isolated bootstrap failed")
        except Exception:
            self.directory.cleanup()
            raise

    def request(self, action):
        child = ProbeChild(self.build, self.root, action)
        status = child.status
        child.close()
        return status

    def start(self, mode):
        if mode not in ("session", "transaction", "recovery"):
            raise RuntimeError("explicit owner mode required")
        child = ProbeChild(self.build, self.root, mode)
        expected = "Ready" if mode == "session" else "Pending"
        if child.status != expected:
            status = child.status
            child.close()
            return status
        if self.owner is not None:
            child.close()
            raise RuntimeError("existing owner must end explicitly")
        self.owner, self.mode = child, mode
        if mode != "session":
            self.assert_pending_owner()
        return child.status

    def assert_pending_owner(self):
        if self.owner is None or self.mode not in ("transaction", "recovery"):
            raise RuntimeError("actual guardian ownership required")
        self.owner.assert_alive()
        if (self.root / "state-v1.journal").read_bytes() != b"\x01":
            raise RuntimeError("durable Pending must precede native installer start")
        if self.request("admit") != "Busy":
            raise RuntimeError("real other process admission not excluded")
        self.owner.assert_alive()

    def end(self, crash=False):
        if self.owner is None:
            raise RuntimeError("actual owner required for explicit end")
        mode = self.mode
        owner, self.owner = self.owner, None
        self.mode = None
        owner.close(crash=crash)
        status = self.request("admit")
        expected = "Ready" if mode == "session" else "Pending"
        if status != expected:
            raise RuntimeError("owner end changed durable gate state")
        return status

    def begin_native(self):
        self.assert_pending_owner()
        self.native_in_flight = True

    def assertions_complete(self):
        # Test lifecycle bookkeeping, not VerifiedHealth or SDK quiescence.
        self.assert_pending_owner()
        self.native_in_flight = False

    def retain_unknown(self):
        self.directory.retain()
        self.build.retain_unknown()

    def close(self):
        if self.native_in_flight:
            self.retain_unknown()
        try:
            if self.owner:
                self.end()
        finally:
            # This removes our fresh CI/test fixture, never repairs Pending or customer state.
            self.directory.cleanup()


def windows_guardian(product):
    require_ci("win32")
    if product not in ("customer-desk", "support-console"):
        raise RuntimeError("separately identified MSI probe product required")
    with ProbeBuild() as build:
        gate = NativeGate(build)
        messages = queue.Queue(maxsize=1)
        def read():
            while True:
                line = sys.stdin.readline(257)
                messages.put(line)
                if not line or len(line) > 256:
                    return
        threading.Thread(target=read, daemon=True).start()
        def send(status):
            print(json.dumps({"status": status}), flush=True)
        try:
            send("Ready")
            while True:
                line = messages.get(timeout=240)
                if not line or len(line) > 256 or not line.endswith("\n"):
                    raise RuntimeError("bounded explicit guardian protocol required")
                message = json.loads(line)
                if not isinstance(message, dict) or set(message) != {"op"}:
                    raise RuntimeError("exact protocol shape required")
                op = message["op"]
                if op in ("session", "transaction", "recovery"):
                    send(gate.start(op))
                elif op == "admit":
                    if gate.owner:
                        gate.owner.assert_alive()
                    send(gate.request("admit"))
                elif op == "pending-owner":
                    gate.assert_pending_owner()
                    send("Busy")
                elif op == "native-starting":
                    gate.begin_native()
                    # MSI's original later downgrade/uninstall cases can also
                    # fail after the guarded phase. Keep this Pending root for
                    # the entire disposable runner lifetime, including success.
                    gate.retain_unknown()
                    send("Busy")
                elif op == "assertions-complete":
                    gate.assertions_complete()
                    send("Busy")
                elif op == "mutation-unknown":
                    gate.retain_unknown()
                    send("Pending")
                elif op in ("end", "crash"):
                    send(gate.end(crash=op == "crash"))
                elif op == "close":
                    if gate.owner:
                        raise RuntimeError("explicit owner end required before close")
                    send("Closed")
                    break
                else:
                    raise RuntimeError("unknown isolated guardian operation")
        finally:
            gate.close()


class SourceTests(unittest.TestCase):
    def test_original_transaction_and_no_health_authority(self):
        native = text(ROOT / "src/ongrow_update/native_apply_tests.rs")
        self.assertIn("let mut lock = os::fixture_acquire(path, true)?;\n    lock.mark_pending()?;\n    Ok(Transaction { lock })", native)
        self.assertIn("pending_recovery_lease(os::fixture_acquire(path, true)?)", native)
        self.assertEqual(len(re.findall(r"#\[test\]\s*fn (?!native_child)\w+", native)), 8)
        for forbidden in ("VerifiedHealth", "mark_ready", "commit_healthy", "thread::sleep", "killall"):
            self.assertNotIn(forbidden, native)
        self.assertIn("self.child.kill()", native)
        self.assertIn("self.child.wait()", native)
        self.assertIn("recv_timeout(Duration::from_secs(300))", native)

    def test_cfg_and_original_module_are_required(self):
        self.assertIn("#[cfg(not(ongrow_native_apply_probe))]", text(FIXTURE))
        self.assertIn('#[path = "../../../src/ongrow_update/session_gate.rs"]', text(FIXTURE))
        self.assertIn("#[cfg(all(test, ongrow_native_apply_probe, any(target_os", text(ROOT / "src/ongrow_update/session_gate.rs"))
        for platform in ("darwin", "win32", "linux"):
            with self.assertRaises(RuntimeError):
                # Local source tests cannot silently become native installer tests.
                with mock.patch.dict(os.environ, {"GITHUB_ACTIONS": "false"}):
                    require_ci(platform)

    def test_both_native_workflows_run_gate_before_installers(self):
        for platform, installer in (("windows", "test_ongrow_windows_msi_lifecycle.ps1"),
                                    ("macos", "test_ongrow_sparkle_probe.py --integration")):
            workflow = text(ROOT / f".github/workflows/ongrow-autoupdate-{platform}-lab.yml")
            self.assertIn("dtolnay/rust-toolchain@e97e2d8cc328f1b50210efc529dca0028893a2d9", workflow)
            self.assertIn("toolchain: '1.81.0'", workflow)
            self.assertLess(workflow.index("scripts/test_ongrow_native_apply.py"), workflow.index(installer))
            self.assertIn("persist-credentials: false", workflow)
            self.assertIn("contents: read", workflow)
            for forbidden in ("pull_request_target", "secrets."):
                self.assertNotIn(forbidden, workflow)

    def test_windows_never_restarts_msi_after_an_inflight_cleanup_timeout(self):
        script = text(ROOT / "scripts/test_ongrow_windows_msi_lifecycle.ps1")
        invoke = script.split("function Invoke-Msi(", 1)[1].split("function Read-Guardian", 1)[0]
        self.assertIn("if ($script:NativeMsiInFlight) { throw 'Previous MSI still in flight; refusing any further MSI start' }", invoke)
        self.assertLess(invoke.index("if ($script:NativeMsiInFlight)"), invoke.index("Start-Process"))
        cleanup = script.split("foreach ($v in @(3,2,1)) {", 1)[1].split("foreach ($item in $sharedSentinels)", 1)[0]
        self.assertIn("if ($script:NativeMsiInFlight) { break }", cleanup)
        self.assertLess(cleanup.index("if ($script:NativeMsiInFlight)"), cleanup.index("try { Invoke-Msi"))


class NativeTests(unittest.TestCase):
    def test_native_twice_missing_input_and_ordinary_app(self):
        with ProbeBuild() as build:
            names = re.findall(r"#\[test\]\s*fn (\w+)", text(ROOT / "src/ongrow_update/native_apply_tests.rs"))
            listing = build.run(["--", "--list"])
            self.assertEqual(listing.returncode, 0, listing.stderr)
            for name in names:
                self.assertIn(PREFIX + name + ": test", listing.stdout)
            for serial in (False, True):
                environment = build.environment.copy()
                arguments = [PREFIX, "--", "--nocapture"]
                if serial:
                    environment["RUST_TEST_THREADS"] = "1"
                    arguments.append("--test-threads=1")
                else:
                    environment.pop("RUST_TEST_THREADS", None)
                result = build.run(arguments, environment)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertIn("9 passed; 0 failed; 0 ignored", result.stdout)
                self.assertIn("NATIVE_APPLY_GATE_PASS", result.stdout)
                print(("serial" if serial else "normal") + "\n" + result.stdout)
            # Exercise the same pipe adapter used around the CI native installers.
            gate = NativeGate(build)
            try:
                self.assertEqual(gate.start("session"), "Ready")
                self.assertEqual(gate.start("transaction"), "Busy")
                self.assertEqual(gate.end(), "Ready")
                self.assertEqual(gate.start("transaction"), "Pending")
                gate.assert_pending_owner()
                self.assertEqual(gate.end(crash=True), "Pending")
                self.assertEqual(gate.start("recovery"), "Pending")
                gate.assert_pending_owner()
                self.assertEqual(gate.end(), "Pending")
            finally:
                gate.close()
            print("NATIVE_APPLY_ADAPTER_REAL_CHILD_PASS")
            missing = build.environment.copy()
            missing.pop("ONGROW_NATIVE_APPLY_TEST_ROOT")
            result = build.run(["--", "--exact", PREFIX + names[0]], missing, timeout=30)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("isolated native apply test root", result.stdout + result.stderr)
            print("MISSING_NATIVE_APPLY_INPUT_REJECTED")
            app = ('#![allow(dead_code)]\nextern crate self as hbb_common;\n'
                   '#[cfg(target_os = "macos")] pub extern crate libc;\n'
                   'mod ongrow_update {\n#[path = ' + json.dumps(str(ROOT / "src/ongrow_update/session_gate.rs")) +
                   ']\nmod session_gate;\n}\n')
            (build.scratch / "app.rs").write_text(app, encoding="utf-8", errors="strict")
            (build.scratch / "Cargo.toml").write_text(build.original + '\n[lib]\npath = "app.rs"\n',
                                                   encoding="utf-8", errors="strict")
            ordinary = missing.copy()
            ordinary["RUSTFLAGS"] = CHECK_CFG
            result = build.run(["--", "--list"], ordinary)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertNotIn("native_apply_tests::", result.stdout)
            self.assertNotIn("pending_recovery_tests::", result.stdout)
            result = build.run(["--", "--test-threads=1"], ordinary)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertIn("1 passed; 0 failed; 0 ignored", result.stdout)
            print("ORDINARY_APP_NO_NATIVE_APPLY_INPUT: PASS")


class RetentionTests(unittest.TestCase):
    def test_guardian_context_exit_and_finalizers_preserve_real_pending_root(self):
        build = ProbeBuild()
        paths = []
        try:
            with build:
                gate = NativeGate(build)
                paths = [Path(build.scratch_directory.name)]
                if build.state_directory:
                    paths.append(Path(build.state_directory.name))
                self.assertEqual(gate.start("transaction"), "Pending")
                gate.begin_native()
                gate.close()
                root = gate.root
                self.assertTrue(gate.directory.retained)
            # Actual ProbeBuild context exit and TemporaryDirectory finalizers
            # have run or are detached, not merely mocked cleanup calls.
            del gate.directory
            del build.scratch_directory
            if build.state_directory:
                del build.state_directory
            gc.collect()
            self.assertTrue(root.is_dir())
            self.assertEqual(gate.request("admit"), "Pending")
            self.assertEqual((root / "state-v1.journal").read_bytes(), b"\x01")
            print("NATIVE_PENDING_ROOT_CONTEXT_RETENTION_PASS")
        finally:
            for path in paths:
                # Retention regression owns these synthetic roots; no installer
                # or SDK helper was launched. Dispose only after asserting them.
                if path.exists():
                    shutil.rmtree(path)

    def test_mac_caller_timeout_decoder_and_pipe_failures_retain_all_owned_roots(self):
        import test_ongrow_sparkle_probe as sparkle
        failures = (subprocess.TimeoutExpired("synthetic-caller", 120),
                    UnicodeDecodeError("utf-8", b"\xff", 0, 1, "synthetic invalid byte"),
                    BrokenPipeError("synthetic caller pipe"))
        for failure in failures:
            with self.subTest(failure=type(failure).__name__):
                paths = []
                build = ProbeBuild()
                task = None
                try:
                    with FixtureDirectory(prefix="retention-mac-", dir=ROOT / "target") as fixture, build:
                        task = Path(fixture.name)
                        (task / "probe-owned-by-test").write_bytes(b"untouched synthetic probe")
                        build.track_native_fixture(fixture)
                        gate = NativeGate(build)
                        self.assertEqual(gate.start("transaction"), "Pending")
                        caller = mock.Mock(returncode=None)
                        caller.poll.side_effect = [None, None, None]
                        caller.communicate.side_effect = failure
                        with mock.patch.object(sparkle, "_start_cli", return_value=caller):
                            with self.assertRaises(type(failure)):
                                sparkle.native_cli(["synthetic-no-installer"], fixture, gate)
                        caller.kill.assert_called_once()
                        caller.wait.assert_called_once_with(timeout=10)
                        gate.close()
                        paths = [task, Path(build.scratch_directory.name)]
                        if build.state_directory:
                            paths.append(Path(build.state_directory.name))
                        root = gate.root
                    del fixture
                    del gate.directory
                    del build.scratch_directory
                    if build.state_directory:
                        del build.state_directory
                    gc.collect()
                    self.assertTrue(task.is_dir())
                    self.assertEqual((task / "probe-owned-by-test").read_bytes(), b"untouched synthetic probe")
                    self.assertEqual(gate.request("admit"), "Pending")
                    self.assertTrue(root.is_dir())
                finally:
                    for path in paths:
                        if path.exists():
                            shutil.rmtree(path)
        # The six original, unguarded native SDK cases use the same caller
        # helper and retain the whole fixture even without a Transaction.
        paths = []
        build = ProbeBuild()
        try:
            with FixtureDirectory(prefix="retention-original-", dir=ROOT / "target") as fixture, build:
                build.track_native_fixture(fixture)
                task = Path(fixture.name)
                paths = [task, Path(build.scratch_directory.name)]
                if build.state_directory:
                    paths.append(Path(build.state_directory.name))
                caller = mock.Mock(returncode=None)
                caller.poll.return_value = None
                caller.communicate.side_effect = BrokenPipeError("synthetic legacy caller pipe")
                with mock.patch.object(sparkle, "_start_cli", return_value=caller):
                    with self.assertRaises(BrokenPipeError):
                        sparkle.native_cli(["synthetic-no-installer"], fixture)
            del fixture
            gc.collect()
            self.assertTrue(task.is_dir())
            self.assertTrue(paths[1].is_dir())
            print("MAC_ALL_FIXTURE_TIMEOUT_DECODER_PIPE_RETENTION_PASS")
        finally:
            for path in paths:
                if path.exists():
                    shutil.rmtree(path)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--ci-windows-guardian", choices=("customer-desk", "support-console"))
    args, remaining = parser.parse_known_args()
    if args.ci_windows_guardian:
        if remaining:
            parser.error("guardian accepts no extra commands or paths")
        windows_guardian(args.ci_windows_guardian)
    else:
        unittest.main(argv=[__file__, *remaining])
