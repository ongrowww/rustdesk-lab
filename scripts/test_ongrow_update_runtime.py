#!/usr/bin/env python3
"""Run original Rust sources against real ephemeral loopback TLS. Never installs tools."""
import http.server
import json
import os
from pathlib import Path
import re
import shutil
import socket
import ssl
import stat
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "scripts/fixtures/ongrow_update_runtime_probe"
PUBLIC_FIXTURES = frozenset(("public.key", "manifest.json", "manifest.sig", "payload.bin"))


def read_app_sources(paths):
    for path in paths:
        if "ongrow_update" not in path.parts:
            yield path, path.read_text(encoding="utf-8", errors="strict")


def validate_public_fixtures(environment):
    directory = environment.get("ONGROW_RELEASE_TEST_FIXTURE_DIR")
    if directory is None:
        if environment.get("GITHUB_ACTIONS") == "true" or environment.get("CI"):
            raise RuntimeError("CI requires the exact public producer fixture artifact")
        return False
    try:
        root = Path(directory)
        if root.is_symlink() or not root.is_dir():
            raise RuntimeError("Public fixture directory must be regular and not a symlink")
        entries = list(root.iterdir())
        if {entry.name for entry in entries} != PUBLIC_FIXTURES:
            raise RuntimeError("Public fixture artifact must contain exactly four named files")
        for entry in entries:
            metadata = entry.lstat()
            if not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1:
                raise RuntimeError("Public fixture files must be regular, unlinked and not symlinks")
        if (root / "public.key").stat().st_size != 32 or (root / "manifest.sig").stat().st_size != 64:
            raise RuntimeError("Public fixture key and signature lengths are invalid")
    except OSError:
        raise RuntimeError("Public fixture artifact is unavailable") from None
    return True


def tool_environment():
    environment = os.environ.copy()
    if sys.platform == "darwin" and environment.get("GITHUB_ACTIONS") != "true":
        toolchain = Path.home() / ".rustup/toolchains/1.81.0-aarch64-apple-darwin/bin"
        if not (toolchain / "cargo").is_file():
            raise RuntimeError("Existing Rust 1.81.0 toolchain missing; no installation allowed")
        environment["PATH"] = str(toolchain) + os.pathsep + environment["PATH"]
    for key in ("CARGO_ENCODED_RUSTFLAGS", "ONGROW_PRODUCT_ROLE", "ONGROW_UPDATE_SESSION_GATE"):
        environment.pop(key, None)
    environment["RUSTFLAGS"] = "--check-cfg=cfg(ongrow_update_runtime_probe) --check-cfg=cfg(ongrow_session_gate_probe) --cfg ongrow_update_runtime_probe"
    return environment


def openssl_tool():
    candidates = [shutil.which("openssl"), "/opt/homebrew/opt/openssl@3/bin/openssl",
                  r"C:\Program Files\Git\usr\bin\openssl.exe"]
    for candidate in candidates:
        if candidate and Path(candidate).is_file():
            result = subprocess.run([candidate, "version"], capture_output=True, text=True,
                                    encoding="utf-8", errors="strict", timeout=10)
            if result.returncode == 0 and result.stdout.startswith("OpenSSL 3."):
                return str(Path(candidate).resolve())
    raise RuntimeError("Existing OpenSSL 3 missing; no installation allowed")


class TlsServer(http.server.ThreadingHTTPServer):
    daemon_threads = False
    block_on_close = True
    def __init__(self, root):
        self.root = root
        self.guard = threading.Lock()
        super().__init__(("127.0.0.1", 0), Handler)
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        context.load_cert_chain(root / "server.pem", root / "key.pem")
        self.socket = context.wrap_socket(self.socket, server_side=True)
        self.socket.settimeout(2)


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    def log_message(self, *args):
        pass
    def setup(self):
        super().setup()
        self.connection.settimeout(2)
    def do_GET(self):
        try:
            with self.server.guard:
                requests_path = self.server.root / "requests.json"
                requests = json.loads(requests_path.read_text(encoding="utf-8", errors="strict"))
                requests.append(self.path)
                requests_path.write_text(json.dumps(requests), encoding="utf-8", errors="strict")
                routes = json.loads((self.server.root / "routes.json").read_text(encoding="utf-8", errors="strict"))
            route = routes.get(self.path, {"status": 404, "body": []})
            if self.headers.get("Accept-Encoding") != "identity":
                raise AssertionError("Original transport must request identity encoding")
            time.sleep(min(route.get("delay", 0), 1))
            body = bytes(route.get("body", []))
            self.send_response(route.get("status", 200))
            self.send_header("Connection", "close")
            if "encoding" in route:
                self.send_header("Content-Encoding", route["encoding"])
            if "location" in route:
                self.send_header("Location", route["location"])
            if route.get("chunked"):
                self.send_header("Transfer-Encoding", "chunked")
                if "length" in route:
                    self.send_header("Content-Length", str(route["length"]))
            else:
                self.send_header("Content-Length", str(route.get("length", len(body))))
                if "duplicate_length" in route:
                    self.send_header("Content-Length", str(route["duplicate_length"]))
            self.end_headers()
            if route.get("reset"):
                self.wfile.write(body[:3])
                self.wfile.flush()
                self.connection.shutdown(socket.SHUT_RDWR)
            elif route.get("chunked"):
                for offset in range(0, len(body), 4096):
                    chunk = body[offset:offset + 4096]
                    self.wfile.write(f"{len(chunk):x}\r\n".encode() + chunk + b"\r\n")
                    self.wfile.flush()
                self.wfile.write(b"0\r\n\r\n")
            else:
                self.wfile.write(body)
            self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError, ssl.SSLError, OSError):
            pass  # Expected rejection/cancellation; no secret/path diagnostics.
        finally:
            self.close_connection = True


class RuntimeTests(unittest.TestCase):
    def test_workflow_public_producer_and_native_matrix_contract(self):
        workflow = (ROOT / ".github/workflows/ongrow-update-runtime-lab.yml").read_text(encoding="utf-8", errors="strict")
        producer, native = workflow.split("  public-producer:\n", 1)[1].split("  native-runtime:\n", 1)
        trusted = "if: github.event_name != 'pull_request' || github.event.pull_request.head.repo.full_name == github.repository"
        self.assertIn(trusted, producer)
        self.assertIn(trusted, native)
        self.assertIn("contents: read", workflow)
        self.assertNotIn("secrets.", workflow)
        for job in (producer, native):
            self.assertIn("SOURCE_SHA: ${{ github.event.pull_request.head.sha || github.sha }}", job)
            self.assertIn('test "$(git rev-parse HEAD)" = "$SOURCE_SHA"', job)
            self.assertIn("persist-credentials: false", job)
        self.assertIn("runs-on: ubuntu-22.04", producer)
        self.assertIn("scripts/test_ongrow_release_signing.py", producer)
        self.assertIn('--export-fixtures-dir "$fixture_dir"', producer)
        self.assertIn("validate_public_fixtures(os.environ)", producer)
        self.assertIn("artifact-name=ongrow-runtime-public-%s-%s", producer)
        self.assertIn('"$GITHUB_RUN_ID" "$GITHUB_RUN_ATTEMPT"', producer)
        self.assertIn("actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a", producer)
        self.assertIn("retention-days: 1", producer)
        self.assertIn("if-no-files-found: error", producer)
        paths = producer.split("          path: |\n", 1)[1]
        self.assertEqual([line.strip() for line in paths.splitlines() if line.strip()],
                         ["${{ env.ONGROW_PUBLIC_FIXTURE_DIR }}/" + name
                          for name in ("public.key", "manifest.json", "manifest.sig", "payload.bin")])
        self.assertIn("needs: public-producer", native)
        self.assertIn("os: [macos-14, windows-2022]", native)
        self.assertIn("actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c", native)
        self.assertIn("name: ${{ needs.public-producer.outputs.artifact-name }}", native)
        self.assertIn("path: target/ongrow-runtime-public-fixtures", native)
        self.assertIn("ONGROW_RELEASE_TEST_FIXTURE_DIR: ${{ github.workspace }}/target/ongrow-runtime-public-fixtures", native)
        self.assertNotIn("test_ongrow_release_signing.py", native)
        self.assertNotIn("pattern:", native)
        self.assertNotIn("merge-multiple:", native)
        self.assertNotIn(".pem", producer)

    def test_public_fixture_artifact_validation(self):
        with tempfile.TemporaryDirectory(prefix="ongrow-public-input-") as directory:
            root = Path(directory)
            values = {"public.key": bytes(32), "manifest.sig": bytes(64),
                      "manifest.json": b"{}", "payload.bin": b"synthetic public fixture"}
            for name, value in values.items():
                (root / name).write_bytes(value)
            environment = {"CI": "true", "ONGROW_RELEASE_TEST_FIXTURE_DIR": directory}
            self.assertTrue(validate_public_fixtures(environment))
            extra = root / "unexpected.txt"
            extra.write_bytes(b"synthetic extra public file")
            with self.assertRaisesRegex(RuntimeError, "exactly four"):
                validate_public_fixtures(environment)
            extra.unlink()
            for name in ("public.key", "manifest.sig"):
                (root / name).write_bytes(values[name][:-1])
                with self.assertRaisesRegex(RuntimeError, "lengths"):
                    validate_public_fixtures(environment)
                (root / name).write_bytes(values[name])
            payload = root / "payload.bin"
            payload.unlink()
            with self.assertRaisesRegex(RuntimeError, "exactly four"):
                validate_public_fixtures(environment)
            payload.mkdir()
            with self.assertRaisesRegex(RuntimeError, "regular"):
                validate_public_fixtures(environment)
            payload.rmdir()
            if os.name == "posix":
                payload.symlink_to(root / "manifest.json")
                with self.assertRaisesRegex(RuntimeError, "not symlinks"):
                    validate_public_fixtures(environment)
                payload.unlink()
            payload.write_bytes(values["payload.bin"])
            self.assertTrue(validate_public_fixtures(environment))
        with self.assertRaisesRegex(RuntimeError, "CI requires"):
            validate_public_fixtures({"CI": "true"})
        self.assertFalse(validate_public_fixtures({}))

    def test_boundaries_and_inactive_apps(self):
        source = (ROOT / "src/ongrow_update/runtime.rs").read_text(encoding="utf-8", errors="strict")
        compact = re.sub(r"\s+", "", source)
        for forbidden in ["std::env", "Client::new", "danger_accept", "response.bytes(", "block_on", "tokio::spawn", "process::Command"]:
            self.assertNotIn(re.sub(r"\s+", "", forbidden), compact)
        for expected in ["https_only(true)", "Policy::none()", ".no_gzip()", ".no_brotli()", ".no_deflate()", ".no_zstd()",
                         "checked_add", "verifier.update(&chunk)?", "verifier.finalize()?", "sink.flush().await"]:
            self.assertIn(re.sub(r"\s+", "", expected), compact)
        self.assertLess(compact.index("LastAcceptedSequence::Unknown)"), compact.index("self.scheduler.acquire(manual)"))
        self.assertLess(compact.index("manifest.size>MAX_PAYLOAD_BYTES"), compact.index("self.response(&manifest.download_url"))
        self.assertLess(compact.index("verifier.update(&chunk)?"), compact.index("sink.write_all(&chunk)"))
        for app_source, text in read_app_sources((ROOT / "src").rglob("*.rs")):
            self.assertNotIn("UpdateRuntime", text, str(app_source.relative_to(ROOT)))
        self.assertFalse(any(path.suffix in [".pem", ".key", ".crt", ".exe"] for path in FIXTURE.rglob("*")))

    def test_real_source_scan_uses_strict_utf8_with_cp1252_undefined_byte(self):
        text = "// UTF-8 portability probe: \u0401\nstruct Probe;\n"
        encoded = text.encode("utf-8", errors="strict")
        self.assertIn(0x81, encoded)
        with self.assertRaises(UnicodeDecodeError):
            encoded.decode("cp1252", errors="strict")
        real_read_text = Path.read_text
        checked_paths = []

        def guarded_read(path, *args, **kwargs):
            self.assertEqual(kwargs.get("encoding"), "utf-8")
            self.assertEqual(kwargs.get("errors"), "strict")
            checked_paths.append(path)
            return real_read_text(path, *args, **kwargs)

        with tempfile.TemporaryDirectory(prefix="ongrow-utf8-source-") as directory:
            fixture = Path(directory) / "utf8-probe.rs"
            fixture.write_text(text, encoding="utf-8", errors="strict")
            with mock.patch.object(Path, "read_text", new=guarded_read):
                # Run the actual unchanged boundary checks over repository sources.
                self.test_boundaries_and_inactive_apps()
                self.assertEqual(list(read_app_sources([fixture])), [(fixture, text)])
                fixture.write_bytes(b"\xff")
                with self.assertRaises(UnicodeDecodeError):
                    list(read_app_sources([fixture]))
            self.assertIn(fixture, checked_paths)
            self.assertTrue(any(path.is_relative_to(ROOT / "src") for path in checked_paths))

    def test_native_original_sources_real_tls(self):
        if sys.platform not in ("darwin", "win32"):
            self.fail("Native macOS or Windows required; no silent skip")
        environment = tool_environment()
        validate_public_fixtures(environment)
        version = subprocess.run(["rustc", "--version"], env=environment, capture_output=True,
                                 text=True, encoding="utf-8", errors="strict", timeout=10)
        self.assertEqual(version.returncode, 0)
        self.assertTrue(version.stdout.startswith("rustc 1.81.0 "), "Exact existing Rust 1.81.0 required")
        target = ROOT / "target/ongrow-update-runtime-probe"
        target.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix="build-", dir=target) as build_directory, \
             tempfile.TemporaryDirectory(prefix="ongrow-runtime-tls-") as tls_directory:
            scratch, tls_root = Path(build_directory), Path(tls_directory)
            openssl = openssl_tool()
            # Private TLS key stays outside Git, never read by Python/model or exported.
            commands = [
                ["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1", "-subj", "/CN=Runtime test CA",
                 "-addext", "basicConstraints=critical,CA:TRUE", "-addext", "keyUsage=critical,keyCertSign,cRLSign",
                 "-keyout", str(tls_root / "ca-key.pem"), "-out", str(tls_root / "cert.pem")],
                ["req", "-new", "-newkey", "rsa:2048", "-nodes", "-subj", "/CN=localhost",
                 "-keyout", str(tls_root / "key.pem"), "-out", str(tls_root / "server.csr")],
                ["x509", "-req", "-in", str(tls_root / "server.csr"), "-CA", str(tls_root / "cert.pem"),
                 "-CAkey", str(tls_root / "ca-key.pem"), "-CAcreateserial", "-days", "1", "-sha256",
                 "-extfile", str(tls_root / "leaf-ext.cnf"), "-out", str(tls_root / "server.pem")],
            ]
            (tls_root / "leaf-ext.cnf").write_text("basicConstraints=critical,CA:FALSE\nsubjectAltName=DNS:localhost\n"
                                                 "keyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n",
                                                 encoding="utf-8", errors="strict")
            for arguments in commands:
                result = subprocess.run([openssl] + arguments, capture_output=True, timeout=20)
                self.assertEqual(result.returncode, 0, "Ephemeral TLS certificate generation failed")
            (tls_root / "routes.json").write_text("{}", encoding="utf-8", errors="strict")
            (tls_root / "requests.json").write_text("[]", encoding="utf-8", errors="strict")
            server = TlsServer(tls_root)
            thread = threading.Thread(target=server.serve_forever, kwargs={"poll_interval": 0.1})
            thread.start()
            try:
                environment["ONGROW_RUNTIME_PROBE_ROOT"] = str(tls_root)
                environment["ONGROW_RUNTIME_PROBE_ORIGIN"] = f"https://localhost:{server.server_port}"
                # This isolated probe must contact only its loopback server. The
                # production builder retains existing proxy conventions.
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
                                         text=True, encoding="utf-8", errors="strict", timeout=300)
                self.assertEqual(listing.returncode, 0, listing.stderr)
                # Discovery is evidence, not a successful empty test command.
                expected = re.findall(r"#\[test\]\s*fn (\w+)",
                                      (ROOT / "src/ongrow_update/tests.rs").read_text(encoding="utf-8", errors="strict"))
                self.assertGreaterEqual(len(expected), 13)
                for name in expected:
                    self.assertIn(f"ongrow_update::tests::{name}: test", listing.stdout)
                native = "ongrow_update::runtime::tests::native::original_transport_real_tls_success_and_rejections"
                self.assertIn(native + ": test", listing.stdout)
                for name in ["streaming_size_hash_and_overflow_boundaries", "scheduler_initial_success_retry_parallel_drop_backward_and_ranges",
                             "trusted_policy_rejects_plain_http_bad_keys_and_ambiguous_locations", "deadline_request_prefix_contract"]:
                    self.assertIn(f"ongrow_update::runtime::tests::{name}: test", listing.stdout)
                result = subprocess.run(command + ["--", "--test-threads=1", "--nocapture"], env=environment,
                                        capture_output=True, text=True, encoding="utf-8", errors="strict", timeout=300)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertIn("NATIVE_TLS_ORIGINAL_TRANSPORT_PASS", result.stdout)
                self.assertNotIn("ignored", result.stdout.split("test result:", 1)[0])
                print(result.stdout)
                missing = environment.copy()
                missing.pop("ONGROW_RUNTIME_PROBE_ROOT", None)
                result = subprocess.run(command + ["--", "--exact", native], env=missing,
                                        capture_output=True, text=True, encoding="utf-8", errors="strict", timeout=30)
                self.assertNotEqual(result.returncode, 0, "Missing probe input must fail")
                self.assertIn("native TLS probe root", result.stdout + result.stderr)
                if "ONGROW_RELEASE_TEST_FIXTURE_DIR" not in environment:
                    print("OPEN: producer interoperability fixtures absent locally, native verifier cases ran")
            finally:
                server.shutdown()
                server.server_close()
                thread.join(timeout=5)
                self.assertFalse(thread.is_alive(), "TLS server thread did not stop")


if __name__ == "__main__":
    unittest.main(verbosity=2)
