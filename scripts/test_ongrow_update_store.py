#!/usr/bin/env python3
"""Test actual protected-store handles and original TLS transport, never real apps."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import unittest

from test_ongrow_update_runtime import TlsServer, openssl_tool, tool_environment
from test_ongrow_update_session_gate import windows_probe_state_directory

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "scripts/fixtures/ongrow_update_store_probe"
ORIGINAL = {
    "session_gate.rs": "be8a5881ed24ee1f9ea098201bd4fde2392d8f9b5f55054dc70d0336543c015e",
    "session_gate/macos.rs": "fec9556d364ccafb83068267ff3a01c0fd62c4199d9ba553651390ec3f44da6e",
    "session_gate/windows.rs": "dedac670ae074e120eac2da986bfd1d8ccbe4bd9716042d74fd409ee0f78c3df",
    "runtime.rs": "0e320975137c96aacc7029a9e80339738afdfccdcda61dcb0a861fb60296e980",
}


def addition(path, expected):
    raw = path.read_bytes()
    start = b"// ONGROW_STORE_ADDITIONS_BEGIN\n"
    end = b"// ONGROW_STORE_ADDITIONS_END\n"
    if raw.count(start) != 1 or raw.count(end) != 1:
        raise AssertionError("Exactly one full-line addition marker pair required")
    index = raw.index(start)
    stop = raw.index(end)
    if stop <= index or (index and raw[index - 1:index] != b"\n"):
        raise AssertionError("Addition markers must be separate ordered lines")
    original = raw[:index] + raw[stop + len(end):]
    if hashlib.sha256(original).hexdigest() != expected:
        raise AssertionError("Original source bytes changed outside additive block")
    block = raw[index + len(start):stop].decode("utf-8", errors="strict")
    if not block.strip():
        raise AssertionError("Empty addition block is not implementation evidence")
    return block


class SourceTests(unittest.TestCase):
    def test_autocrlf_checkout_preserves_exact_protected_source_bytes(self):
        environment = {name: value for name, value in os.environ.items()
                       if not name.upper().startswith("GIT_")}
        environment.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull,
                           GIT_ATTR_NOSYSTEM="1")
        target = ROOT / "target/ongrow-update-store-checkout"
        target.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix="checkout-", dir=target) as directory:
            scratch = Path(directory)
            hooks, template, checkout = (scratch / name for name in ("hooks", "template", "repo"))
            for path in (hooks, template, checkout):
                path.mkdir()

            def git(cwd, *arguments):
                return subprocess.run(
                    ["git", "-c", "core.attributesFile=" + os.devnull,
                     "-c", "core.hooksPath=" + str(hooks), "-c", "commit.gpgSign=false",
                     "-c", "tag.gpgSign=false", "-c", "user.name=Fixture",
                     "-c", "user.email=fixture@example.invalid", *arguments],
                    cwd=cwd, env=environment, capture_output=True, timeout=30, check=True)

            canonical = {}
            for name in ORIGINAL:
                relative = "src/ongrow_update/" + name
                raw = git(ROOT, "show", "HEAD:" + relative).stdout
                raw.decode("utf-8", errors="strict")
                self.assertNotIn(b"\r\n", raw)
                canonical[relative] = raw
            git(checkout, "init", "--template=" + str(template))
            git(checkout, "config", "core.autocrlf", "true")
            attributes = (ROOT / ".gitattributes").read_bytes()
            attributes.decode("utf-8", errors="strict")
            files = {**canonical, ".gitattributes": attributes, "control.rs": b"// checkout control\n"}
            for relative, raw in files.items():
                path = checkout / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(raw)
            git(checkout, "add", "--", *files)
            for relative in files:
                (checkout / relative).unlink()
            git(checkout, "checkout-index", "--all", "--force")
            self.assertEqual((checkout / "control.rs").read_bytes(), b"// checkout control\r\n")
            for name, digest in ORIGINAL.items():
                relative = "src/ongrow_update/" + name
                with self.subTest(source=name):
                    self.assertEqual((checkout / relative).read_bytes(), canonical[relative])
                    addition(checkout / relative, digest)

    def assert_rejected_source(self, transform, message):
        with tempfile.TemporaryDirectory(prefix="ongrow-store-source-") as directory:
            for name, digest in ORIGINAL.items():
                with self.subTest(source=name):
                    path = Path(directory) / "source.rs"
                    path.write_bytes(transform((ROOT / "src/ongrow_update" / name).read_bytes()))
                    with self.assertRaisesRegex(AssertionError, message):
                        addition(path, digest)

    def test_crlf_source_is_rejected_without_normalization(self):
        self.assert_rejected_source(lambda raw: raw.replace(b"\n", b"\r\n"),
                                    "Exactly one full-line addition marker pair required")

    def test_changed_bytes_outside_addition_are_rejected(self):
        self.assert_rejected_source(lambda raw: b"// changed original bytes\n" + raw,
                                    "Original source bytes changed outside additive block")

    def test_duplicate_addition_markers_are_rejected(self):
        for marker in (b"// ONGROW_STORE_ADDITIONS_BEGIN\n", b"// ONGROW_STORE_ADDITIONS_END\n"):
            with self.subTest(marker=marker):
                self.assert_rejected_source(lambda raw: raw + marker,
                                            "Exactly one full-line addition marker pair required")

    def test_missing_addition_markers_are_rejected(self):
        for marker in (b"// ONGROW_STORE_ADDITIONS_BEGIN\n", b"// ONGROW_STORE_ADDITIONS_END\n"):
            with self.subTest(marker=marker):
                self.assert_rejected_source(lambda raw: raw.replace(marker, b""),
                                            "Exactly one full-line addition marker pair required")

    def test_original_hashes_and_reused_trust(self):
        blocks = {name: addition(ROOT / "src/ongrow_update" / name, digest)
                  for name, digest in ORIGINAL.items()}
        mac = blocks["session_gate/macos.rs"]
        win = blocks["session_gate/windows.rs"]
        for required in ("root(product)?", "trusted_directory(path, owner)?", "inspect(", "open_at(",
                         "libc::O_EXCL", "libc::LOCK_NB", "libc::F_FULLFSYNC"):
            self.assertIn(required, mac)
        for required in ("root(product)?", "directory(&path, &user, product)?", "inspect(",
                         "FILE_FLAG_OPEN_REPARSE_POINT", "create_new(create)", "LockFileEx("):
            self.assertIn(required, win)
        self.assertIn(".share_mode(1)", win)
        self.assertIn("self.checked_child(Child::StageLock, false)?", win)
        self.assertIn("offset.Anonymous.Anonymous.Offset = 1", win)
        self.assertEqual(win.count("&mut store_lock_offset()"), 2)
        for block in (mac, win):
            for forbidden in ("fn inspect(", "fn trusted_sid(", "fn trusted_ace_sid(", "getpwuid", "SHGetKnownFolderPath",
                              "std::env", "remove_file", "remove_dir", "set_permissions", "SetSecurity", "chown("):
                self.assertNotIn(forbidden, block)
            self.assertIn("#[cfg(all(test, ongrow_update_store_probe))]", block)
        tls = blocks["runtime.rs"]
        self.assertIn("#[cfg(all(test, ongrow_update_store_probe))]", tls)
        self.assertIn("client_builder().add_root_certificate(certificate)", tls)
        for forbidden in ("std::env", "danger_accept", "Client::new", "pub fn"):
            self.assertNotIn(forbidden, tls)

    def test_private_fixed_identity_and_no_cleanup_or_authority(self):
        source = (ROOT / "src/ongrow_update/protected_store.rs").read_text(encoding="utf-8", errors="strict")
        handles = addition(ROOT / "src/ongrow_update/session_gate.rs", ORIGINAL["session_gate.rs"])
        for name in ("accepted-sequence-v1", "staging-v1.lock", "stage-v1.payload"):
            self.assertEqual(handles.count('"' + name + '"'), 1)
        for forbidden in ("remove_file", "remove_dir", "read_dir", "set_permissions", "std::env", "PathBuf",
                          "pub fn", "fn advance", "fn commit", "fn recover", "fn discard", "pub(crate) fn path"):
            self.assertNotIn(forbidden, source)
        for required in ("Product::CustomerDesk => session_gate::Product::CustomerDesk",
                         "Product::SupportConsole => session_gate::Product::SupportConsole",
                         "input.writer.flush()", "input._context.root.sync_file(&input.writer)", "drop(input.writer)",
                         "input._context.root.same(&reader", "verify_manifest(raw, signature", "candidate.payload_verifier()",
                         "tokio::task::spawn_blocking", "_lease: Arc<StageLease>, _context: Arc<StoreContext>"):
            self.assertIn(required, source)
        self.assertLess(source.index("drop(input.writer)"), source.index("let reader = input._context.root.read"))
        initialization = source.split("Ok(PendingStage {", 1)[1].split("})", 1)[0]
        for field in ("writer: None", "identity: None", "work: None"):
            self.assertIn(field, initialization)
        before_download = source.split("fn begin_stage", 1)[1].split("pub(crate) async fn download", 1)[0]
        self.assertIn("self.context.root.read(Child::Payload)", before_download)
        self.assertNotIn("create(Child::Payload)", before_download)
        ticket = source.split("pub(crate) struct SealedStageTicket", 1)[1]
        for forbidden in ("pub(crate) reader:", "pub(crate) signature:", "fn write", "fn path", "fn construct"):
            self.assertNotIn(forbidden, ticket)
        mod = (ROOT / "src/ongrow_update/mod.rs").read_text(encoding="utf-8", errors="strict")
        self.assertIn('#[cfg(any(target_os = "macos", target_os = "windows"))]\npub(crate) mod protected_store;', mod)

    def test_actual_job_factories_whole_capture_and_ordered_guards(self):
        source = (ROOT / "src/ongrow_update/protected_store.rs").read_text(encoding="utf-8", errors="strict")
        for name, following in (("write_job", "fn seal_job"), ("seal_job", "impl PendingStage")):
            factory = source.split("fn " + name + "(", 1)[1].split(following, 1)[0]
            self.assertRegex(factory, r"move \|\| \{\s*let mut input = input;")
            for forbidden in ("let lease = input.", "let context = input.", "let _lease = input."):
                self.assertNotIn(forbidden, factory)
        self.assertIn("spawn_blocking(write_job(input))", source)
        self.assertIn("spawn_blocking(seal_job(input, transfer))", source)
        for structure, writer, lease, context in (("StageJobInput", "writer: Option<File>", "lease: Arc<StageLease>", "context: Arc<StoreContext>"),
                                                   ("WorkResult", "writer: File", "_lease: Arc<StageLease>", "_context: Arc<StoreContext>")):
            body = source.split("struct " + structure + " {", 1)[1].split("\n}", 1)[0]
            self.assertTrue(body.lstrip().startswith(writer))
            observer = '#[cfg(all(test, ongrow_update_store_probe))]\n    observer: Option<tests::DropObserver>'
            self.assertIn(observer, body)
            self.assertLess(body.index(writer), body.index(observer))
            self.assertLess(body.index(observer), body.index("identity:"))
            self.assertLess(body.index("identity:"), body.index(lease))
            self.assertLess(body.index(lease), body.index(context))
        tests = (ROOT / "src/ongrow_update/protected_store_tests.rs").read_text(encoding="utf-8", errors="strict")
        for name in ("actual_write_closure_drop_closes_writer_before_native_guards",
                     "actual_seal_closure_drop_closes_writer_before_native_guards",
                     "actual_early_seal_error_closes_writer_before_native_guards"):
            self.assertIn("fn " + name + "()", tests)
        observer = tests.split("impl Drop for DropObserver", 1)[1].split("pub(super) struct CreationPause", 1)[0]
        for required in ("libc::F_GETFD", "libc::EBADF", "GetHandleInformation", "ERROR_INVALID_HANDLE", 'child(&self.path, "busy")'):
            self.assertIn(required, observer)
        self.assertLess(observer.index("GetHandleInformation(HANDLE"), observer.index('child(&self.path, "busy")'))
        for forbidden in ("from_raw_fd", "from_raw_handle", ".upgrade()", "CloseHandle"):
            self.assertNotIn(forbidden, observer)

    def test_probe_pins_match_existing_and_workflows_are_inactive(self):
        original = (ROOT / "scripts/fixtures/ongrow_update_runtime_probe/Cargo.toml").read_text(encoding="utf-8", errors="strict")
        fixture = (FIXTURE / "Cargo.toml").read_text(encoding="utf-8", errors="strict")
        self.assertEqual(original.split("[dependencies]", 1)[1], fixture.split("[dependencies]", 1)[1])
        workflow = (ROOT / ".github/workflows/ongrow-update-store-lab.yml").read_text(encoding="utf-8", errors="strict")
        for required in ("contents: read", "os: [macos-14, windows-2022]", "toolchain: '1.81.0'",
                         "github.event.pull_request.head.repo.full_name == github.repository", "persist-credentials: false",
                         "SOURCE_SHA: ${{ github.event.pull_request.head.sha || github.sha }}",
                         'test "$(git rev-parse HEAD)" = "$SOURCE_SHA"', "python scripts/test_ongrow_update_store.py"):
            self.assertIn(required, workflow)
        for forbidden in ("secrets.", "upload-artifact", "sudo", "flutter", "Set-Acl"):
            self.assertNotIn(forbidden, workflow)
        runtime = (ROOT / ".github/workflows/ongrow-update-runtime-lab.yml").read_text(encoding="utf-8", errors="strict")
        self.assertIn("python scripts/test_ongrow_update_session_gate.py", runtime)
        self.assertIn("python scripts/test_ongrow_update_store.py --source-only", runtime)
        self.assertIn('git diff --exit-code a620d13809b94cc2baa06bf77a4a741769ef413f "$SOURCE_SHA"', runtime)
        for active in (workflow, runtime):
            paths = active.split("    paths:\n", 1)[1].split("\npermissions:", 1)[0]
            self.assertIn("      - '.gitattributes'\n", paths)
            self.assertIn("      - 'scripts/test_ongrow_update_store.py'\n", paths)


class NativeTests(unittest.TestCase):
    def test_original_native_store_real_tls_and_handles(self):
        self.assertIn(sys.platform, ("darwin", "win32"), "Native host required; no silent skip")
        environment = tool_environment()
        environment["RUSTFLAGS"] = ("--check-cfg=cfg(ongrow_session_gate_probe) "
                                    "--check-cfg=cfg(ongrow_update_runtime_probe) "
                                    "--check-cfg=cfg(ongrow_update_store_probe) --cfg ongrow_update_store_probe")
        version = subprocess.run(["rustc", "--version"], env=environment, capture_output=True,
                                 encoding="utf-8", errors="strict", timeout=10, check=True)
        self.assertTrue(version.stdout.startswith("rustc 1.81.0 "))
        target = ROOT / "target/ongrow-update-store-probe"
        target.mkdir(parents=True, exist_ok=True)
        state_directory = windows_probe_state_directory() if os.name == "nt" else tempfile.TemporaryDirectory(prefix="state-", dir=target)
        self.addCleanup(state_directory.cleanup)
        with tempfile.TemporaryDirectory(prefix="build-", dir=target) as build_directory, \
             tempfile.TemporaryDirectory(prefix="ongrow-store-tls-") as tls_directory:
            scratch, tls = Path(build_directory), Path(tls_directory)
            (tls / "leaf-ext.cnf").write_text("basicConstraints=critical,CA:FALSE\nsubjectAltName=DNS:localhost\n"
                                             "keyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n",
                                             encoding="utf-8", errors="strict")
            tool = openssl_tool()
            commands = [
                ["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1", "-subj", "/CN=Store test CA",
                 "-addext", "basicConstraints=critical,CA:TRUE", "-addext", "keyUsage=critical,keyCertSign,cRLSign",
                 "-keyout", str(tls / "ca-key.pem"), "-out", str(tls / "cert.pem")],
                ["req", "-new", "-newkey", "rsa:2048", "-nodes", "-subj", "/CN=localhost",
                 "-keyout", str(tls / "key.pem"), "-out", str(tls / "server.csr")],
                ["x509", "-req", "-in", str(tls / "server.csr"), "-CA", str(tls / "cert.pem"),
                 "-CAkey", str(tls / "ca-key.pem"), "-CAcreateserial", "-days", "1", "-sha256",
                 "-extfile", str(tls / "leaf-ext.cnf"), "-out", str(tls / "server.pem")],
            ]
            for arguments in commands:
                result = subprocess.run([tool] + arguments, capture_output=True, timeout=20)
                self.assertEqual(result.returncode, 0, "Ephemeral test TLS generation failed")
            for name, value in (("routes.json", "{}"), ("requests.json", "[]")):
                (tls / name).write_text(value, encoding="utf-8", errors="strict")
            server = TlsServer(tls)
            thread = threading.Thread(target=server.serve_forever, kwargs={"poll_interval": 0.1})
            thread.start()
            try:
                environment["ONGROW_STORE_TEST_ROOT"] = str(Path(state_directory.name).resolve())
                environment["ONGROW_STORE_TLS_ROOT"] = str(tls)
                environment["ONGROW_STORE_TLS_ORIGIN"] = f"https://localhost:{server.server_port}"
                environment["NO_PROXY"] = "localhost,127.0.0.1"
                environment["no_proxy"] = environment["NO_PROXY"]
                environment["CARGO_TARGET_DIR"] = str(target / "build")
                manifest = (FIXTURE / "Cargo.toml").read_text(encoding="utf-8", errors="strict")
                manifest += "\n[lib]\npath = " + json.dumps(str(FIXTURE / "lib.rs")) + "\n"
                (scratch / "Cargo.toml").write_text(manifest, encoding="utf-8", errors="strict")
                shutil.copyfile(ROOT / "Cargo.lock", scratch / "Cargo.lock")
                command = ["cargo", "test", "--lib", "--manifest-path", str(scratch / "Cargo.toml")]
                if environment.get("GITHUB_ACTIONS") != "true":
                    command.append("--offline")
                listing = subprocess.run(command + ["--", "--list"], env=environment, capture_output=True,
                                         encoding="utf-8", errors="strict", timeout=300)
                self.assertEqual(listing.returncode, 0, listing.stderr)
                source = (ROOT / "src/ongrow_update/protected_store_tests.rs").read_text(encoding="utf-8", errors="strict")
                names = re.findall(r"#\[test\]\s*fn (\w+)", source)
                self.assertGreaterEqual(len(names), 8)
                prefix = "ongrow_update::protected_store::tests::"
                for name in names:
                    self.assertIn(prefix + name + ": test", listing.stdout)
                result = subprocess.run(command + [prefix, "--", "--test-threads=1", "--nocapture"], env=environment,
                                        capture_output=True, encoding="utf-8", errors="strict", timeout=300)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertIn("NATIVE_STORE_TLS_HANDLE_PASS", result.stdout)
                self.assertNotIn("ignored", result.stdout.split("test result:", 1)[0])
                print(result.stdout)
                missing = environment.copy()
                missing.pop("ONGROW_STORE_TEST_ROOT", None)
                result = subprocess.run(command + ["--", "--exact", prefix + "state_reopen_and_identity_binding"], env=missing,
                                        capture_output=True, encoding="utf-8", errors="strict", timeout=30)
                self.assertNotEqual(result.returncode, 0, "Missing native input must fail")
                self.assertIn("isolated store test root", result.stdout + result.stderr)
                ordinary = environment.copy()
                ordinary["RUSTFLAGS"] = "--check-cfg=cfg(ongrow_session_gate_probe) --check-cfg=cfg(ongrow_update_runtime_probe) --check-cfg=cfg(ongrow_update_store_probe)"
                app = '#![allow(dead_code)]\nextern crate self as hbb_common;\npub extern crate sodiumoxide;\npub extern crate tokio;\npub extern crate libc;\npub fn get_app_name() -> String { "RustDesk".to_owned() }\n#[path = ' + json.dumps(str(ROOT / "src/ongrow_update/mod.rs")) + ']\nmod ongrow_update;\n'
                (scratch / "app-shape.rs").write_text(app, encoding="utf-8", errors="strict")
                (scratch / "Cargo.toml").write_text(manifest.rsplit("\n[lib]\n", 1)[0] + '\n[lib]\npath = "app-shape.rs"\n', encoding="utf-8", errors="strict")
                discovery = subprocess.run(command + ["--", "--list"], env=ordinary, capture_output=True,
                                           encoding="utf-8", errors="strict", timeout=300)
                self.assertEqual(discovery.returncode, 0, discovery.stderr)
                self.assertNotIn("protected_store::tests::", discovery.stdout)
            finally:
                server.shutdown()
                server.server_close()
                thread.join(timeout=5)
                self.assertFalse(thread.is_alive(), "TLS server did not stop")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--source-only", action="store_true")
    arguments = parser.parse_args()
    suite = unittest.defaultTestLoader.loadTestsFromTestCase(SourceTests)
    if not arguments.source_only:
        suite.addTests(unittest.defaultTestLoader.loadTestsFromTestCase(NativeTests))
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    raise SystemExit(not result.wasSuccessful())
