#!/usr/bin/env python3
"""Real Ed25519 lab-producer tests. Export only public synthetic fixtures."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

import ongrow_release_signing as signing

ROOT = Path(__file__).resolve().parents[1]
OPENSSL = None
SHA = "0123456789abcdef0123456789abcdef01234567"
PUBLIC_FIXTURES = ("public.key", "manifest.json", "manifest.sig", "payload.bin")


def temporary_directory():
    # Private user-owned fixtures outside Git, without relying on writable
    # /tmp ancestors. Canonicalize macOS's /var alias before path validation.
    return tempfile.TemporaryDirectory(prefix="ongrow-release-fixture-", dir=Path.home().resolve())


class ReleaseSigningTests(unittest.TestCase):
    def setUp(self):
        self.directory = temporary_directory()
        self.root = Path(self.directory.name).resolve()
        self.keys = self.root / "keys"
        self.fingerprint = signing.init_key(self.keys, OPENSSL)
        self.key = self.keys / signing.PRIVATE_NAME
        self.public = self.keys / signing.PUBLIC_NAME
        self.payload = self.root / "ongrow-desk-Setup.exe"
        self.payload.write_bytes(b"synthetic unsigned lab payload, not a PE acceptance")
        self.output = self.root / "release-9"
        self.options = dict(payload=self.payload, key_file=self.key, public_key_file=self.public,
                            openssl=OPENSSL, output_dir=self.output, product="customer-desk",
                            platform="windows-x64", channel="lab", release_sequence=9,
                            previous_sequence=8, version="1.4.9", source_sha=SHA,
                            issued_at=90, expires_at=110, origin="https://updates.example.test",
                            path_prefix="/releases/", now=100)

    def tearDown(self):
        self.directory.cleanup()

    def sign(self, **changes):
        return signing.sign_release(**(self.options | changes))

    def rejected(self, **changes):
        with self.assertRaises((signing.SigningError, OSError)):
            self.sign(**changes)
        self.assertFalse(self.output.exists())

    def verify(self, raw, signature, public=None):
        message = self.root / "verification-message"
        public_der = self.root / "verification-public.der"
        detached = self.root / "verification.sig"
        message.write_bytes(signing.DOMAIN + raw)
        public_der.write_bytes(signing.SPKI_PREFIX + (self.public.read_bytes() if public is None else public))
        detached.write_bytes(signature)
        return signing.openssl_operation(OPENSSL, ["pkeyutl", "-verify", "-rawin", "-pubin", "-keyform", "DER",
                                                  "-inkey", str(public_der), "-in", str(message), "-sigfile", str(detached)])

    def test_key_generation_is_exclusive_and_private_bytes_never_enter_python(self):
        self.assertEqual(self.fingerprint, hashlib.sha256(self.public.read_bytes()).hexdigest())
        self.assertEqual(self.public.stat().st_size, 32)
        self.assertGreater(self.key.stat().st_size, 0)
        if hasattr(os, "getuid"):
            self.assertEqual(stat.S_IMODE(self.keys.stat().st_mode), 0o700)
            self.assertEqual(stat.S_IMODE(self.key.stat().st_mode), 0o600)
            self.assertEqual(stat.S_IMODE(self.public.stat().st_mode), 0o600)
        before = signing.source_state(self.key.stat())
        with self.assertRaisesRegex(signing.SigningError, "Existing output"):
            signing.init_key(self.keys, OPENSSL)
        self.assertEqual(signing.source_state(self.key.stat()), before)
        real_read_bytes, real_read_text = Path.read_bytes, Path.read_text

        def guarded_bytes(path):
            self.assertNotEqual(path.suffix, ".pem", "Private PEM must never be read by Python")
            return real_read_bytes(path)

        def guarded_text(path, *args, **kwargs):
            self.assertNotEqual(path.suffix, ".pem", "Private PEM must never be read by Python")
            return real_read_text(path, *args, **kwargs)

        with mock.patch.object(Path, "read_bytes", new=guarded_bytes), \
                mock.patch.object(Path, "read_text", new=guarded_text):
            signing.init_key(self.root / "other keys", OPENSSL)
            self.sign()

    def test_real_detached_signature_and_exact_copied_payload_for_both_products(self):
        original = self.payload.read_bytes()
        for index, product in enumerate(("customer-desk", "support-console")):
            output = self.root / f"release-{index}"
            manifest = self.sign(product=product, output_dir=output)
            raw = (output / "manifest.json").read_bytes()
            signature = (output / "manifest.sig").read_bytes()
            self.assertEqual(len(signature), 64)
            self.verify(raw, signature)
            self.assertEqual(json.loads(raw), manifest)
            self.assertEqual(raw, signing.manifest_bytes(manifest))
            self.assertEqual(manifest["schema_version"], 1)
            self.assertEqual(manifest["product"], product)
            self.assertEqual(manifest["size"], len(original))
            self.assertEqual(manifest["sha256"], hashlib.sha256(original).hexdigest())
            self.assertEqual(manifest["download_url"], f"https://updates.example.test/releases/9/{self.payload.name}")
            self.assertEqual((output / self.payload.name).read_bytes(), original)
            self.assertEqual(sorted(p.name for p in output.iterdir()), sorted((self.payload.name, "manifest.json", "manifest.sig")))
        self.assertEqual(self.payload.read_bytes(), original)

    def test_console_zip_is_data_not_installation_approval(self):
        payload = self.root / "ongrow-console.zip"
        payload.write_bytes(b"synthetic ZIP transport")
        manifest = self.sign(payload=payload, product="support-console")
        self.assertEqual(manifest["filename"], payload.name)
        self.assertEqual((self.output / payload.name).read_bytes(), payload.read_bytes())

    def test_msi_is_signable_data_not_product_or_installer_approval(self):
        payload = self.root / "ongrow-desk.msi"
        payload.write_bytes(b"synthetic MSI transport, no MSI build")
        manifest = self.sign(payload=payload)
        self.assertEqual(manifest["filename"], payload.name)
        self.assertEqual((self.output / payload.name).read_bytes(), payload.read_bytes())

    def test_manifest_tampering_domain_and_foreign_public_key_are_rejected(self):
        self.sign()
        raw = (self.output / "manifest.json").read_bytes()
        signature = (self.output / "manifest.sig").read_bytes()
        for altered in (raw + b" ", raw.replace(b'"lab"', b'"stable"')):
            with self.assertRaises(signing.SigningError):
                self.verify(altered, signature)
        with self.assertRaises(signing.SigningError):
            self.verify(raw, bytes([signature[0] ^ 1]) + signature[1:])
        signing.init_key(self.root / "foreign", OPENSSL)
        foreign = self.root / "foreign" / signing.PUBLIC_NAME
        with self.assertRaises(signing.SigningError):
            self.verify(raw, signature, foreign.read_bytes())
        message = self.root / "wrong-domain-message"
        message.write_bytes(raw)
        without_domain = signing.openssl_operation(OPENSSL, ["pkeyutl", "-sign", "-rawin", "-inkey", str(self.key), "-in", str(message)])
        with self.assertRaises(signing.SigningError):
            self.verify(raw, without_domain)

    def test_foreign_or_malformed_public_key_fails_before_output(self):
        signing.init_key(self.root / "foreign", OPENSSL)
        self.rejected(public_key_file=self.root / "foreign" / signing.PUBLIC_NAME)
        for raw in (b"", b"x" * 31, b"x" * 33, b"x" * 64):
            self.public.write_bytes(raw)
            self.rejected()

    def test_u64_time_sequence_product_and_metadata_validation_before_mutation(self):
        invalid = [dict(release_sequence=8), dict(release_sequence=0), dict(previous_sequence=9),
                   dict(product="other"), dict(platform="windows-arm64"), dict(channel="stable"),
                   dict(issued_at=101), dict(expires_at=100), dict(expires_at=89),
                   dict(version=""), dict(version="1" * 65), dict(version="1.4.9+build"),
                   dict(source_sha="A" * 40), dict(source_sha=SHA[:8])]
        for field in ("release_sequence", "previous_sequence", "issued_at", "expires_at"):
            invalid.extend({field: value} for value in (-1, signing.U64_MAX + 1, True, "9"))
        for changes in invalid:
            with self.subTest(changes=changes):
                self.rejected(**changes)
        self.sign(release_sequence=signing.U64_MAX, previous_sequence=signing.U64_MAX - 1,
                  issued_at=0, expires_at=signing.U64_MAX)

    def test_download_origin_and_prefix_reject_normalization_aliases(self):
        for origin in ("http://updates.example.test", "https://updates.example.test:443",
                       "https://Updates.example.test", "https://user@updates.example.test",
                       "https://updates.example.test/", "https://updates.example.test?q=1",
                       "https://updates.example.test#x", "https://updates.example.test:0443",
                       "https://updates.example.test:65536", "https://127.0.0.1",
                       "https://updates..test", "https://updates.example.test."):
            with self.subTest(origin=origin):
                self.rejected(origin=origin)
        for prefix in ("/", "//releases/", "releases/", "/releases", "/releases/../",
                       "/releases//", "/releases%2f/", "/releases\\other/", "/réleases/"):
            with self.subTest(prefix=prefix):
                self.rejected(path_prefix=prefix)

    def test_filename_rules_casefold_and_sensitive_inputs(self):
        for name in ("CON.exe", "nul.exe", "LPT9.exe", "COM1.zip", "payload.exe.", "payload.exe ",
                     "payload.exe:ads", "payload%2eexe", "payload\\other.exe", "MANIFEST.JSON",
                     "manifest.sig", "payload.pem", ".env.exe"):
            path = self.root / name
            path.write_bytes(b"synthetic unsafe fixture")
            with self.subTest(name=name):
                self.rejected(payload=path)
            path.unlink()
        # Do not write a case alias, which would overwrite the original on macOS.
        real_iterdir = Path.iterdir

        def injected(path):
            entries = list(real_iterdir(path))
            if path == self.root:
                entries.append(self.root / self.payload.name.upper())
            return iter(entries)

        with mock.patch.object(Path, "iterdir", new=injected):
            self.rejected()

    def test_symlinks_and_unambiguous_absolute_paths_are_required(self):
        for original, changes in ((self.payload, "payload"), (self.key, "key_file"),
                                  (self.public, "public_key_file")):
            alias = self.root / (changes + "-alias" + original.suffix)
            alias.symlink_to(original)
            self.rejected(**{changes: alias})
        alias = self.root / "parent-alias"
        alias.symlink_to(self.root, target_is_directory=True)
        self.rejected(output_dir=alias / "release")
        dangling = self.root / "dangling"
        dangling.symlink_to(self.root / "missing")
        self.rejected(output_dir=dangling)
        for path in ("relative.exe", "//host/share/payload.exe", str(self.root / ".." / "payload.exe"),
                     str(self.root / "x:ads.exe")):
            self.rejected(payload=path)
        with self.assertRaises(signing.SigningError):
            signing.init_key(alias / "keys-alias", OPENSSL)

    def test_existing_output_overlap_and_git_keys_are_refused(self):
        self.rejected(output_dir=self.payload)
        self.rejected(output_dir=self.keys)
        (self.root / "RELEASE-9").mkdir()
        before = sorted(p.name for p in self.root.iterdir())
        with self.assertRaises(signing.SigningError):
            self.sign()
        self.assertEqual(sorted(p.name for p in self.root.iterdir()), before)
        (self.root / "RELEASE-9").rmdir()
        (self.root / ".git").mkdir()
        self.rejected()
        with self.assertRaises(signing.SigningError):
            signing.init_key(self.root / "new-keys", OPENSSL)

    def test_wrong_owner_unsafe_modes_and_hardlinks_are_refused(self):
        if hasattr(os, "getuid"):
            for path, mode in ((self.key, 0o644), (self.public, 0o644), (self.keys, 0o755),
                               (self.root, 0o777)):
                previous = stat.S_IMODE(path.stat().st_mode)
                path.chmod(mode)
                try:
                    self.rejected()
                finally:
                    path.chmod(previous)  # Only this synthetic fixture is restored.
            metadata = self.key.stat()
            foreign = mock.Mock(st_mode=metadata.st_mode, st_uid=os.getuid() + 1000, st_nlink=1, st_file_attributes=0)
            with self.assertRaisesRegex(signing.SigningError, "owner"):
                signing.safe_metadata(foreign, private=True)
        alias = self.root / "hardlink.exe"
        os.link(self.payload, alias)
        self.rejected()

    def test_missing_or_non_openssl3_tool_fails_closed(self):
        self.rejected(openssl=self.root / "missing-openssl")
        self.rejected(openssl="openssl")
        with mock.patch.object(signing, "openssl_operation", return_value=b"LibreSSL 3.3.6\n"):
            self.rejected()

    def test_homebrew_tool_exception_is_narrow_and_never_changes_data_policy(self):
        tool = Path("/opt/homebrew/Cellar/openssl@3/3.6.4/bin/openssl")
        requested = Path("/opt/homebrew/opt/openssl@3/bin/openssl")
        cellar = Path("/opt/homebrew/Cellar")
        # These are metadata-policy tests, not substitutes for real crypto tests.
        current_uid = os.getuid()
        safe = mock.Mock(st_mode=stat.S_IFDIR | 0o755, st_uid=current_uid, st_gid=80, st_file_attributes=0)
        authorized = mock.Mock(st_mode=stat.S_IFDIR | 0o775, st_uid=current_uid, st_gid=80, st_file_attributes=0)

        def metadata(path):
            return authorized if path == cellar else safe

        with mock.patch.object(Path, "lstat", new=metadata), \
                mock.patch("grp.getgrgid", return_value=mock.Mock(gr_name="admin")):
            signing.check_tool_parents(tool, requested)
            signing.check_tool_parents(tool, tool)
            for other in (Path("/arbitrary/bin/openssl"), Path("/opt/homebrew/opt/other/bin/openssl")):
                with self.assertRaises(signing.SigningError):
                    signing.check_tool_parents(tool, other)
            with self.assertRaises(signing.SigningError):
                signing.check_tool_parents(Path("/opt/homebrew/Cellar/other/3.6.4/bin/openssl"), requested)
            with self.assertRaises(signing.SigningError):
                signing.safe_metadata(authorized, directory=True)
            for mode, owner in ((0o777, current_uid), (0o775, current_uid + 1000),
                                (0o775, 0), (0o2775, current_uid)):
                authorized.st_mode, authorized.st_uid = stat.S_IFDIR | mode, owner
                with self.assertRaises(signing.SigningError):
                    signing.check_tool_parents(tool, requested)
            authorized.st_mode, authorized.st_uid = stat.S_IFDIR | 0o775, current_uid
            safe.st_mode = stat.S_IFDIR | 0o775
            with self.assertRaises(signing.SigningError):
                signing.check_tool_parents(tool, requested)
            safe.st_mode = stat.S_IFDIR | 0o755
            authorized.st_mode = stat.S_IFLNK | 0o775
            with self.assertRaises(signing.SigningError):
                signing.check_tool_parents(tool, requested)
            authorized.st_mode = stat.S_IFDIR | 0o775
        with mock.patch.object(Path, "lstat", new=metadata), \
                mock.patch("grp.getgrgid", return_value=mock.Mock(gr_name="staff")):
            with self.assertRaises(signing.SigningError):
                signing.check_tool_parents(tool, requested)

    def test_source_mutation_during_copy_fails_and_leaves_no_release(self):
        original_fsync = os.fsync
        mutated = False

        def mutate(descriptor):
            nonlocal mutated
            if not mutated:
                self.payload.write_bytes(b"changed synthetic source during copy")
                mutated = True
            return original_fsync(descriptor)

        with mock.patch.object(os, "fsync", side_effect=mutate):
            with self.assertRaisesRegex(signing.SigningError, "Payload source changed"):
                self.sign()
        self.assertTrue(mutated)
        self.assertFalse(self.output.exists())

    def test_signing_failure_cleans_only_owned_files_and_never_marks_complete(self):
        real_operation = signing.openssl_operation

        def fail(tool, arguments, **kwargs):
            if "-sign" in arguments:
                (self.output / "unknown-leftover").write_bytes(b"must survive")
                raise signing.SigningError("OpenSSL operation failed")
            return real_operation(tool, arguments, **kwargs)

        with mock.patch.object(signing, "openssl_operation", side_effect=fail):
            with self.assertRaises(signing.SigningError):
                self.sign()
        self.assertEqual(sorted(p.name for p in self.output.iterdir()), ["unknown-leftover"])
        self.assertEqual((self.output / "unknown-leftover").read_bytes(), b"must survive")

    def test_bad_signature_length_or_verification_failure_leaves_no_bundle(self):
        real_operation = signing.openssl_operation
        for phase in ("length", "verify"):
            def fail(tool, arguments, **kwargs):
                if phase == "length" and "-sign" in arguments:
                    return b"not a detached signature"
                if phase == "verify" and "-verify" in arguments:
                    raise signing.SigningError("OpenSSL operation failed")
                return real_operation(tool, arguments, **kwargs)

            with mock.patch.object(signing, "openssl_operation", side_effect=fail):
                self.rejected()

    def test_keygen_failure_cleans_own_files_without_exposing_diagnostics(self):
        destination = self.root / "failed-keygen"
        marker = b"SYNTHETIC_PRIVATE_DIAGNOSTIC_DO_NOT_PRINT"
        real_run = signing.subprocess.run

        def failed(command, **kwargs):
            if "genpkey" in command:
                kwargs["stdout"].write(marker)
                return subprocess.CompletedProcess(command, 1, stdout=None, stderr=marker)
            return real_run(command, **kwargs)

        with mock.patch.object(signing.subprocess, "run", side_effect=failed):
            with self.assertRaises(signing.SigningError) as error:
                signing.init_key(destination, OPENSSL)
        self.assertNotIn(marker.decode(), str(error.exception))
        self.assertFalse(destination.exists())

    def test_replaced_owned_inode_is_not_cleaned(self):
        owned = signing.OwnedPaths()
        path = self.root / "owned-file"
        owned.write(path, b"owned")
        # Keep the inode alive so replacement cannot reuse it.
        path.rename(self.root / "saved-owned-file")
        path.write_bytes(b"unknown replacement")
        owned.cleanup()
        self.assertEqual(path.read_bytes(), b"unknown replacement")

    def test_cli_help_success_and_failure_have_no_private_output(self):
        cli = [sys.executable, str(ROOT / "scripts/ongrow_release_signing.py")]
        for command in ([], ["init-key"], ["sign"]):
            result = subprocess.run(cli + command + ["--help"], capture_output=True)
            self.assertEqual(result.returncode, 0)
        result = subprocess.run(cli + ["init-key", "--directory", str(self.root / "cli-keys"),
                                       "--openssl", str(OPENSSL)], capture_output=True)
        self.assertEqual(result.returncode, 0)
        self.assertIn(b"public SHA-256 fingerprint=", result.stdout)
        self.assertNotIn(b"PRIVATE KEY", result.stdout + result.stderr)
        result = subprocess.run(cli + ["init-key", "--directory", str(self.keys),
                                       "--openssl", str(OPENSSL)], capture_output=True)
        self.assertEqual(result.returncode, 1)
        self.assertNotIn(b"PRIVATE KEY", result.stdout + result.stderr)
        options = self.options | dict(issued_at=0, expires_at=signing.U64_MAX)
        options.pop("now")
        arguments = [argument for key, value in options.items() for argument in ("--" + key.replace("_", "-"), str(value))]
        result = subprocess.run(cli + ["sign", *arguments], capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        self.assertIn(b"Signed lab bundle", result.stdout)
        self.assertNotIn(b"PRIVATE KEY", result.stdout + result.stderr)

    def test_workflow_transports_exactly_public_fixtures_and_sets_native_gate(self):
        workflow = (ROOT / ".github/workflows/ongrow-support-console-windows-x64.yml").read_text()
        upload = workflow.split("- name: Upload public synthetic release fixtures", 1)[1].split("\n      - name:", 1)[0]
        paths = upload.split("          path: |\n", 1)[1].strip().splitlines()
        self.assertEqual([p.strip() for p in paths], ["${{ env.ONGROW_PUBLIC_FIXTURE_DIR }}/" + name for name in PUBLIC_FIXTURES])
        for name in PUBLIC_FIXTURES:
            self.assertIn(f"${{{{ env.ONGROW_PUBLIC_FIXTURE_DIR }}}}/{name}", upload)
        self.assertNotIn(".pem", upload)
        bridge = workflow.split("- name: Upload bridge artifact", 1)[1].split("\n  build-windows-x64:", 1)[0]
        self.assertNotIn("FIXTURE", bridge)
        self.assertNotIn("public.key", bridge)
        export = workflow.split("- name: Test isolated lab release signing", 1)[1].split("\n      - name:", 1)[0]
        self.assertIn("Path.home().resolve()", export)
        self.assertIn('fixture_dir="$fixture_home/ongrow-release-public-${{ github.run_id }}-${{ github.run_attempt }}"', export)
        self.assertIn('--export-fixtures-dir "$fixture_dir"', export)
        self.assertNotIn("GITHUB_WORKSPACE", export)
        self.assertNotIn("target/", export)
        restore = workflow.split("- name: Restore public synthetic release fixtures", 1)[1].split("\n      - name:", 1)[0]
        for artifact in (upload, restore):
            self.assertIn("name: ongrow-release-public-${{ github.run_id }}-${{ github.run_attempt }}", artifact)
        self.assertIn("path: ${{ env.ONGROW_PUBLIC_FIXTURE_DIR }}", restore)
        windows_job = workflow.split("\n  build-windows-x64:", 1)[1]
        self.assertNotIn("runner.", windows_job.split("    steps:", 1)[0])
        prepare = workflow.split("- name: Require absent public fixture destination", 1)[1].split("\n      - name:", 1)[0]
        self.assertIn('Join-Path $env:RUNNER_TEMP "ongrow-release-public-${{ github.run_id }}-${{ github.run_attempt }}"', prepare)
        self.assertIn('"ONGROW_PUBLIC_FIXTURE_DIR=$fixtureDir" | Out-File -FilePath $env:GITHUB_ENV', prepare)
        self.assertLess(workflow.index("- name: Require absent public fixture destination"), workflow.index("- name: Restore public synthetic release fixtures"))
        self.assertIn('if (Test-Path -LiteralPath $fixtureDir) { throw "Public fixture destination already exists" }', prepare)
        gate = workflow.split("- name: Run OnGROW Rust tests", 1)[1].split("\n      - name:", 1)[0]
        self.assertIn('(Resolve-Path -LiteralPath $env:ONGROW_PUBLIC_FIXTURE_DIR).Path', gate)
        self.assertLess(gate.index("ONGROW_RELEASE_TEST_FIXTURE_DIR"), gate.index("cargo test --locked --lib --features flutter ongrow_update"))
        self.assertIn('if ($LASTEXITCODE -ne 0) { throw "OnGROW signed update tests failed" }', gate)

    def test_public_export_exact_bytes_and_two_exclusive_run_outputs_without_pem_reads(self):
        real_read_bytes, real_read_text = Path.read_bytes, Path.read_text

        def guarded_bytes(path):
            self.assertNotEqual(path.suffix, ".pem")
            return real_read_bytes(path)

        def guarded_text(path, *args, **kwargs):
            self.assertNotEqual(path.suffix, ".pem")
            return real_read_text(path, *args, **kwargs)

        for run in ("run-1-attempt-1", "run-1-attempt-2"):
            destination = self.root / run
            with mock.patch.object(Path, "read_bytes", new=guarded_bytes), \
                    mock.patch.object(Path, "read_text", new=guarded_text), \
                    mock.patch("builtins.print") as printed:
                export_public_fixtures(destination)
            printed.assert_called_once_with("Exported exactly four public synthetic verifier fixtures")
            self.assertEqual(sorted(p.name for p in destination.iterdir()), sorted(PUBLIC_FIXTURES))
            public = (destination / "public.key").read_bytes()
            raw = (destination / "manifest.json").read_bytes()
            signature = (destination / "manifest.sig").read_bytes()
            payload = (destination / "payload.bin").read_bytes()
            self.assertEqual(len(public), 32)
            self.assertEqual(len(signature), 64)
            self.verify(raw, signature, public)
            manifest = json.loads(raw)
            self.assertEqual(manifest["sha256"], hashlib.sha256(payload).hexdigest())
            self.assertEqual(manifest["size"], len(payload))
            before = {p.name: p.read_bytes() for p in destination.iterdir()}
            with self.assertRaisesRegex(signing.SigningError, "Existing output"):
                export_public_fixtures(destination)
            self.assertEqual({p.name: p.read_bytes() for p in destination.iterdir()}, before)

    def test_public_export_refuses_cached_destination_unsafe_parent_and_symlinks_before_keygen(self):
        cached = self.root / "cached-target"
        cached.mkdir()
        marker = cached / "public.key"
        marker.write_bytes(b"foreign cached bytes")
        alias = self.root / "alias"
        alias.symlink_to(cached, target_is_directory=True)
        unsafe = self.root / "unsafe-parent"
        unsafe.mkdir()
        unsafe.chmod(0o775)
        paths = [cached, alias, alias / "output"]
        if hasattr(os, "getuid"):
            paths.append(unsafe / "output")
        with mock.patch.object(signing, "init_key") as keygen:
            for destination in paths:
                with self.subTest(destination=destination.name), self.assertRaises(signing.SigningError):
                    export_public_fixtures(destination)
            keygen.assert_not_called()
        self.assertEqual(marker.read_bytes(), b"foreign cached bytes")
        self.assertEqual(list(unsafe.iterdir()), [])

    def test_public_export_failure_never_removes_foreign_leftover(self):
        destination = self.root / "public-export"
        real_write = signing.OwnedPaths.write

        def fail(owned, path, data):
            if path.parent == destination:
                real_write(owned, path, data)
                (destination / "foreign-leftover").write_bytes(b"must survive")
                raise signing.SigningError("Synthetic export failure")
            return real_write(owned, path, data)

        with mock.patch.object(signing.OwnedPaths, "write", new=fail), self.assertRaises(signing.SigningError):
            export_public_fixtures(destination)
        self.assertEqual([p.name for p in destination.iterdir()], ["foreign-leftover"])
        self.assertEqual((destination / "foreign-leftover").read_bytes(), b"must survive")

    def test_public_export_replaced_directory_or_symlink_is_rejected_without_foreign_cleanup(self):
        real_write = signing.OwnedPaths.write
        for redirected in (False, True):
            destination = self.root / f"public-export-{redirected}"
            saved = self.root / f"saved-export-{redirected}"
            foreign = self.root / f"foreign-export-{redirected}"
            foreign.mkdir()
            (foreign / "public.key").write_bytes(b"foreign replacement")

            def replace(owned, path, data):
                real_write(owned, path, data)
                if path.parent == destination:
                    destination.rename(saved)
                    if redirected:
                        destination.symlink_to(foreign, target_is_directory=True)
                    else:
                        foreign.rename(destination)

            with mock.patch.object(signing.OwnedPaths, "write", new=replace), self.assertRaises(signing.SigningError):
                export_public_fixtures(destination)
            self.assertEqual((destination / "public.key").read_bytes(), b"foreign replacement")
            self.assertEqual([p.name for p in destination.iterdir()], ["public.key"])
            self.assertEqual([p.name for p in saved.iterdir()], ["public.key"])

    def test_public_export_replaced_file_is_rejected_and_not_cleaned(self):
        destination = self.root / "public-export"
        real_write = signing.OwnedPaths.write

        def replace(owned, path, data):
            real_write(owned, path, data)
            if path == destination / "public.key":
                path.rename(self.root / "saved-public.key")
                path.write_bytes(b"foreign replacement")

        with mock.patch.object(signing.OwnedPaths, "write", new=replace), self.assertRaisesRegex(signing.SigningError, "file changed"):
            export_public_fixtures(destination)
        self.assertEqual([p.name for p in destination.iterdir()], ["public.key"])
        self.assertEqual((destination / "public.key").read_bytes(), b"foreign replacement")


def check_export_directory(destination, expected_identity):
    signing.local_path(destination / "public.key")
    metadata = destination.lstat()
    signing.safe_metadata(metadata, directory=True, private=True)
    if signing.identity(metadata) != expected_identity:
        raise signing.SigningError("Public fixture directory changed")


def export_public_fixtures(destination):
    destination = signing.local_path(destination)
    signing.absent(destination)
    owned = signing.OwnedPaths()
    destination_identity = None
    with temporary_directory() as temporary:
        root = Path(temporary).resolve()
        keys = root / "keys"
        signing.init_key(keys, OPENSSL)
        payload = root / "synthetic-lab.zip"
        payload.write_bytes(b"synthetic signed lab transport; no installer")
        output = root / "release"
        signing.sign_release(payload=payload, key_file=keys / signing.PRIVATE_NAME,
                             public_key_file=keys / signing.PUBLIC_NAME, openssl=OPENSSL,
                             output_dir=output, product="customer-desk", platform="windows-x64", channel="lab",
                             release_sequence=9, previous_sequence=8, version="1.4.9", source_sha=SHA,
                             issued_at=90, expires_at=110, origin="https://updates.example.test",
                             path_prefix="/releases/", now=100)
        try:
            signing.local_path(destination)
            if not destination.parent.exists():
                owned.mkdir(destination.parent)
            owned.mkdir(destination)
            destination_identity = signing.identity(destination.lstat())
            for name, source in (("public.key", keys / signing.PUBLIC_NAME), ("manifest.json", output / "manifest.json"),
                                 ("manifest.sig", output / "manifest.sig"), ("payload.bin", output / payload.name)):
                check_export_directory(destination, destination_identity)
                owned.write(destination / name, source.read_bytes())
                check_export_directory(destination, destination_identity)
            if sorted(p.name for p in destination.iterdir()) != sorted(PUBLIC_FIXTURES):
                raise signing.SigningError("Unexpected fixture export entry")
            for path, inode in owned.files:
                metadata = signing.regular_file(path)
                if signing.identity(metadata) != inode:
                    raise signing.SigningError("Public fixture file changed")
        except Exception:
            try:
                if destination_identity is not None:
                    check_export_directory(destination, destination_identity)
                else:
                    signing.local_path(destination)
            except (signing.SigningError, OSError):
                pass  # A replaced/redirected directory is no longer our cleanup target.
            else:
                owned.cleanup()
            raise
    print("Exported exactly four public synthetic verifier fixtures")


def main():
    global OPENSSL
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--openssl", required=True)
    parser.add_argument("--export-fixtures-dir")
    args = parser.parse_args()
    try:
        OPENSSL = signing.openssl_tool(args.openssl)
    except (signing.SigningError, OSError):
        parser.exit(1, "Tests require an existing safe OpenSSL 3 executable\n")
    result = unittest.TextTestRunner(verbosity=2).run(unittest.defaultTestLoader.loadTestsFromTestCase(ReleaseSigningTests))
    if not result.wasSuccessful() or result.skipped:
        raise SystemExit(1)
    if args.export_fixtures_dir:
        try:
            export_public_fixtures(args.export_fixtures_dir)
        except (signing.SigningError, OSError):
            parser.exit(1, "Public fixture export refused\n")


if __name__ == "__main__":
    main()
