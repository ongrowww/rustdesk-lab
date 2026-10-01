#!/usr/bin/env python3
"""Synthetic packaging fixtures, no Windows installation or global dependencies."""

from __future__ import annotations

import ast
import hashlib
import importlib.util
import json
import os
import re
import runpy
from pathlib import Path
import shlex
import shutil
import subprocess
import struct
import sys
import tempfile
import types
import unittest
from unittest import mock

import package_ongrow_windows_setup as setup

SHA = "a" * 40
VERSION = "1.4.9"


class SetupTests(unittest.TestCase):
    def setUp(self):
        self.assertEqual(len(SHA), 40)
        self.assertIsNotNone(re.fullmatch(r"[0-9a-f]{40}", SHA))
        self.directory = tempfile.TemporaryDirectory(prefix="ongrow-setup-fixture-")
        self.root = Path(self.directory.name).resolve()
        self.source = self.root / "Desk payload with spaces"
        self.output = self.root / "Setup output with spaces"
        self.source.mkdir()
        for required in setup.REQUIRED:
            path = self.source / required
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"fixture")
        (self.source / "ongrow-build-provenance.txt").write_text(f"source={SHA}\n")
        binary = bytearray(256)
        binary[:2] = b"MZ"
        struct.pack_into("<I", binary, 0x3c, 128)
        binary[128:132] = b"PE\0\0"
        struct.pack_into("<H", binary, 132, 0x8664)
        for identity in ("OnGROW GmbH", "OnGROW Support Desk", "ongrow_support_desk", setup.DESK_EXE):
            binary.extend((identity + "\0").encode("utf-16le"))
        (self.source / setup.DESK_EXE).write_bytes(binary)
        (self.source / "librustdesk.dll").write_bytes(b"Rust core: OnGROW Support Desk")
        (self.root / "diagnostic.zip").write_bytes(b"keep ZIP")

    def tearDown(self):
        self.directory.cleanup()

    def validate(self, **kwargs):
        return setup.validate(kwargs.get("source", self.source), kwargs.get("output", self.output),
                              kwargs.get("version", VERSION), kwargs.get("source_sha", SHA))

    def assert_rejected_without_writes(self, **kwargs):
        with self.assertRaises(ValueError):
            self.validate(**kwargs)
        self.assertFalse(self.output.exists())

    def test_complete_tree_version_provenance_and_no_overlap(self):
        self.assertEqual(len(self.validate()), len(setup.REQUIRED))
        for required in setup.REQUIRED:
            original = (self.source / required).read_bytes()
            (self.source / required).unlink()
            self.assert_rejected_without_writes()
            (self.source / required).write_bytes(original)
        self.assert_rejected_without_writes(version="1.4.8")
        self.assert_rejected_without_writes(version="../../bad")
        self.assert_rejected_without_writes(source_sha=SHA[:8])
        self.assert_rejected_without_writes(source_sha="b" * 40)
        for output in (self.source, self.source / "nested", self.root):
            self.assert_rejected_without_writes(output=output)

    def test_foreign_private_and_windows_unsafe_names_before_mutation(self):
        for name in ("rustdesk.exe", "OnGROW Support Console.exe", "RuntimeBroker_rustdesk.exe",
                     "data/private/token", "data/.env.test", "logs/client.log", "data/CON.txt",
                     "data/trailing.", "data/key.pem"):
            path = self.source / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"rejected")
            self.assert_rejected_without_writes()
            path.unlink()
            # Remove empty synthetic sensitive directories, never the real repository.
            if path.parent != self.source and not any(path.parent.iterdir()):
                path.parent.rmdir()

    def test_casefold_duplicate_is_seen_on_every_filesystem(self):
        original = self.source / "data/icudtl.dat"
        original_bytes = original.read_bytes()
        duplicate = self.source / "data/ICUDTL.DAT"
        metadata = original.lstat()
        real_lstat, real_stat = Path.lstat, Path.stat
        inspected = []

        def fixture_lstat(path, *args, **kwargs):
            if path in (original, duplicate):
                inspected.append(path.name)
            if path == duplicate:
                return metadata
            return real_lstat(path, *args, **kwargs)

        def fixture_stat(path, *args, **kwargs):
            if path == duplicate:
                return metadata
            return real_stat(path, *args, **kwargs)

        entries = list(os.walk(self.source))
        injected = []
        for directory, directories, files in entries:
            if Path(directory) == self.source / "data":
                self.assertIn("icudtl.dat", files)
                files = files + ["ICUDTL.DAT"]
            injected.append((directory, directories, files))
        # Supply regular-file metadata for the injected name even on a
        # case-sensitive host; never create or overwrite the real ICU file.
        with mock.patch.object(setup.os, "walk", return_value=injected), \
                mock.patch.object(Path, "lstat", new=fixture_lstat), \
                mock.patch.object(Path, "stat", new=fixture_stat):
            with self.assertRaisesRegex(ValueError, "Case-insensitive duplicate"):
                self.validate()
        self.assertEqual(inspected, ["ICUDTL.DAT", "icudtl.dat"])
        self.assertEqual(original.read_bytes(), original_bytes)
        self.assertFalse(self.output.exists())

    @unittest.skipUnless(hasattr(os, "symlink"), "Symlinks unsupported")
    def test_symlink_input_output_and_parent_are_refused(self):
        link = self.source / "data/link"
        try:
            link.symlink_to(self.source / "data/icudtl.dat")
        except OSError as error:
            self.skipTest(f"Runner cannot create symlink: {error}")
        self.assert_rejected_without_writes()
        link.unlink()
        alias = self.root / "alias"
        alias.symlink_to(self.source, target_is_directory=True)
        self.assert_rejected_without_writes(source=alias)
        self.assert_rejected_without_writes(source=alias / "data")
        self.assert_rejected_without_writes(output=alias / "output")

    def test_path_rules_match_rust(self):
        self.assertEqual(setup.relative_path(".\\data\\nested\\asset"), "data/nested/asset")
        for path in ("../escape", "C:/escape", "\\\\host\\share", "a:ads", "LPT9.txt",
                     "a ", "././a", "data/../escape", "data/NUL.txt"):
            with self.assertRaises(ValueError, msg=path):
                setup.relative_path(path)

    def test_pe_architecture_and_role_are_required_before_packaging(self):
        path = self.source / setup.DESK_EXE
        original = path.read_bytes()
        for invalid in (b"portable upstream", original.replace(b"MZ", b"NZ", 1),
                        original.replace("OnGROW Support Desk\0".encode("utf-16le"),
                                         "OnGROW Support Fake\0".encode("utf-16le"))):
            path.write_bytes(invalid)
            self.assert_rejected_without_writes()
        invalid = bytearray(original)
        struct.pack_into("<H", invalid, 132, 0x14c)
        path.write_bytes(invalid)
        self.assert_rejected_without_writes()
        path.write_bytes(original)
        (self.source / "librustdesk.dll").write_bytes(b"foreign Rust core")
        self.assert_rejected_without_writes()

    def test_packaging_reuses_generator_feature_target_without_shell(self):
        real_generator = setup.generator()
        # Compression is supplied by CI's installed brotli. The unit fixture exercises
        # framing and compilation arguments with no local Python dependency install.
        fake_brotli = types.SimpleNamespace(compress=lambda data, quality: b"compressed:" + data)

        def compile_mock(command, check):
            self.assertTrue(check)
            self.assertEqual(command, ["cargo", "build", "--locked", "--release", "--target",
                                      setup.TARGET, "--features", setup.FEATURE])
            packer = Path.cwd()
            embedded = (packer / "data.bin").read_bytes()
            self.assertTrue(embedded.startswith(b"rustdesk"))
            self.assertTrue(embedded.endswith(b"rustdesk" + setup.DESK_EXE.encode()))
            self.assertIn(b"./data/flutter_assets/AssetManifest.json", embedded)
            self.assertIn(b"./LICENCE", embedded)
            binary = packer / "target" / setup.TARGET / "release/rustdesk-portable-packer.exe"
            binary.parent.mkdir(parents=True)
            binary.write_bytes(b"MZunsigned-fixture")

        cwd = Path.cwd()
        with mock.patch.object(setup, "generator", return_value=real_generator), \
                mock.patch.dict(sys.modules, brotli=fake_brotli), \
                mock.patch.object(real_generator.subprocess, "run", side_effect=compile_mock):
            binary = setup.package(self.source, self.output, VERSION, SHA)
        self.assertEqual(Path.cwd(), cwd)
        self.assertEqual(binary.name, f"ongrow-support-desk-{VERSION}-windows-x64-{SHA[:8]}-Setup.exe")
        metadata = json.loads(binary.with_name(binary.name + ".json").read_text())
        self.assertEqual(metadata["product"], "customer-desk")
        self.assertEqual(metadata["platform"], "windows-x64")
        self.assertEqual(metadata["source_sha"], SHA)
        self.assertEqual(metadata["size"], binary.stat().st_size)
        self.assertEqual(metadata["sha256"], hashlib.sha256(binary.read_bytes()).hexdigest())
        self.assertTrue(metadata["unsigned_lab"])
        self.assertIn(metadata["sha256"], binary.with_name(binary.name + ".sha256").read_text())
        self.assertEqual((self.root / "diagnostic.zip").read_bytes(), b"keep ZIP")
        with mock.patch.object(setup, "generator", side_effect=AssertionError("Compiler must not start")):
            with self.assertRaisesRegex(ValueError, "refusing overwrite"):
                setup.package(self.source, self.output, VERSION, SHA)

    def test_cli_help_and_invalid_input_exit_before_writes(self):
        command = [sys.executable, str(setup.ROOT / "scripts/package_ongrow_windows_setup.py")]
        self.assertEqual(subprocess.run(command + ["--help"], capture_output=True).returncode, 0)
        result = subprocess.run(command + ["--source", str(self.source), "--output", str(self.output),
                                "--version", VERSION, "--source-sha", "invalid"], capture_output=True)
        self.assertEqual(result.returncode, 1)
        self.assertFalse(self.output.exists())

    def test_upstream_generator_executable_and_target_contract(self):
        generator = setup.generator()
        with mock.patch.object(generator.subprocess, "run") as run:
            generator.build_portable(str(self.root), "", None)
            run.assert_called_once_with(["cargo", "build", "--locked", "--release"], check=True)

    def test_upstream_windows_builder_moves_only_the_standalone_packer(self):
        # Execute the actual function body without importing host-specific
        # build.py globals or reimplementing its packaging decisions.
        tree = ast.parse((setup.ROOT / "build.py").read_text())
        function = next(node for node in tree.body
                        if isinstance(node, ast.FunctionDef) and node.name == "build_flutter_windows")
        module = ast.Module(body=[function], type_ignores=[])
        generator_path = str(setup.ROOT / "libs/portable/generate.py")
        fake_brotli = types.SimpleNamespace(compress=lambda data, quality: data)
        build_dir = "flutter/build/windows/x64/runner/Release/"
        caller_cwd = Path.cwd()
        for existing_portable in (False, True):
            for skip_pack in (False, True):
                with self.subTest(existing_portable=existing_portable, skip_pack=skip_pack):
                    root = self.root / f"builder-{existing_portable}-{skip_pack}"
                    payload = root / build_dir
                    payload.mkdir(parents=True)
                    (payload / "rustdesk.exe").write_bytes(b"plain RustDesk fixture")
                    portable = root / "libs/portable"
                    portable.mkdir(parents=True)
                    stale = root / "target/release/rustdesk-portable-packer.exe"
                    stale.parent.mkdir(parents=True)
                    stale.write_bytes(b"stale root packer")
                    (stale.parent / "librustdesk.dll").write_bytes(b"core fixture")
                    display = stale.parent / "deps/dylib_virtual_display.dll"
                    display.parent.mkdir()
                    display.write_bytes(b"display fixture")
                    previous = root / "rustdesk_portable.exe"
                    if existing_portable:
                        previous.write_bytes(b"previous portable")
                    fresh = portable / "target/release/rustdesk-portable-packer.exe"
                    commands = []

                    def compile_packer(command, check):
                        self.assertEqual(Path.cwd(), portable)
                        self.assertEqual(command, ["cargo", "build", "--locked", "--release"])
                        self.assertTrue(check)
                        self.assertTrue((portable / "data.bin").read_bytes().endswith(b"rustdesk./rustdesk.exe"))
                        fresh.parent.mkdir(parents=True)
                        fresh.write_bytes(b"fresh standalone packer")

                    def system2(command):
                        commands.append((Path.cwd(), command))
                        if command.startswith("python3 ./generate.py "):
                            argv = [generator_path, *shlex.split(command)[2:]]
                            with mock.patch.object(sys, "argv", argv), \
                                    mock.patch.dict(sys.modules, brotli=fake_brotli), \
                                    mock.patch("subprocess.run", side_effect=compile_packer) as compiler:
                                runpy.run_path(generator_path, run_name="__main__")
                            compiler.assert_called_once()

                    namespace = dict(os=os, shutil=shutil, system2=system2,
                                     skip_cargo=False, flutter_build_dir_2=build_dir)
                    exec(compile(module, str(setup.ROOT / "build.py"), "exec"), namespace)
                    real_replace, real_rename = os.replace, os.rename
                    try:
                        os.chdir(root)
                        with mock.patch.object(os, "replace", wraps=real_replace) as replace, \
                                mock.patch.object(os, "rename", wraps=real_rename) as rename:
                            namespace["build_flutter_windows"](VERSION, "flutter", skip_pack)
                        self.assertEqual(Path.cwd(), root)
                    finally:
                        os.chdir(caller_cwd)
                    self.assertEqual(Path.cwd(), caller_cwd)
                    self.assertTrue(stale.is_file(), "The stale root packer must remain untouched")
                    self.assertEqual(stale.read_bytes(), b"stale root packer")
                    self.assertEqual((payload / display.name).read_bytes(), b"display fixture")
                    expected_commands = [(root, "cargo build --locked --features flutter --lib --release"),
                                         (root / "flutter", "flutter build windows --release")]
                    installer = root / f"rustdesk-{VERSION}-install.exe"
                    if skip_pack:
                        self.assertFalse(installer.exists())
                        self.assertFalse(fresh.exists())
                        self.assertFalse((portable / "data.bin").exists())
                        self.assertEqual(previous.exists(), existing_portable)
                        if existing_portable:
                            self.assertEqual(previous.read_bytes(), b"previous portable")
                        replace.assert_not_called()
                        rename.assert_not_called()
                    else:
                        expected_commands.extend([
                            (portable, "pip3 install -r requirements.txt"),
                            (portable, f"python3 ./generate.py -f ../../{build_dir} -o . -e ../../{build_dir}/rustdesk.exe")])
                        self.assertEqual(installer.read_bytes(), b"fresh standalone packer")
                        self.assertFalse(fresh.exists())
                        self.assertFalse(previous.exists())
                        source = "./libs/portable/target/release/rustdesk-portable-packer.exe"
                        destination = "./rustdesk_portable.exe"
                        final_move = mock.call(destination, f"./rustdesk-{VERSION}-install.exe")
                        if existing_portable:
                            replace.assert_called_once_with(source, destination)
                            self.assertEqual(rename.call_args_list, [final_move])
                        else:
                            replace.assert_not_called()
                            self.assertEqual(rename.call_args_list, [mock.call(source, destination), final_move])
                    self.assertEqual(commands, expected_commands)

    def test_upstream_cli_preserves_build_py_executable_paths(self):
        source = self.root / "upstream with spaces"
        source.mkdir()
        executable = source / "rustdesk.exe"
        executable.write_bytes(b"upstream fixture")
        generator_path = str(setup.ROOT / "libs/portable/generate.py")
        fake_brotli = types.SimpleNamespace(compress=lambda data, quality: data)
        for index, options in enumerate(([], ["-e", str(executable)],
                                          ["-e", os.path.relpath(executable, Path.cwd())])):
            output = self.root / f"upstream output {index}"
            output.mkdir()
            argv = [generator_path, "-f", str(source), "-o", str(output), *options]
            with mock.patch.object(sys, "argv", argv), mock.patch.dict(sys.modules, brotli=fake_brotli), \
                    mock.patch("subprocess.run") as compiler:
                runpy.run_path(generator_path, run_name="__main__")
            compiler.assert_called_once_with(["cargo", "build", "--locked", "--release"], check=True)
            self.assertTrue((output / "data.bin").read_bytes().endswith(b"rustdesk./rustdesk.exe"))

    def test_ci_uses_trusted_head_existing_desk_and_offline_setup(self):
        workflow = (setup.ROOT / ".github/workflows/ongrow-lab-windows-x64.yml").read_text()
        self.assertIn("pull_request:", workflow)
        self.assertNotIn("pull_request_target:", workflow)
        self.assertEqual(workflow.count("github.event.pull_request.head.repo.full_name == github.repository"), 2)
        self.assertIn('SOURCE_SHA: "${{ github.event.pull_request.head.sha || github.sha }}"', workflow)
        self.assertEqual(workflow.count("ref: ${{ env.SOURCE_SHA }}"), 2)
        self.assertNotIn("$GITHUB_SHA", workflow)
        self.assertNotIn("$env:GITHUB_SHA", workflow)
        self.assertIn("test \"$(git rev-parse HEAD)\" = \"$SOURCE_SHA\"", workflow)
        self.assertIn("if ((git rev-parse HEAD) -ne $env:SOURCE_SHA)", workflow)
        self.assertIn("os.environ['SOURCE_SHA']", workflow)
        self.assertEqual(workflow.count("python .\\build.py --portable --flutter --skip-portable-pack"), 1)
        self.assertLess(workflow.index("Unexpected ProductName"), workflow.index("Test and package complete unsigned Setup"))
        self.assertLess(workflow.index("Trust-anchor fingerprint mismatch"), workflow.index("package_ongrow_windows_setup.py"))
        self.assertIn("Copy-Item LICENCE $artifact", workflow)
        self.assertIn("cargo test --locked --manifest-path libs/portable/Cargo.toml --features ongrow-support-desk", workflow)
        # Source coverage of CI fail-closed ordering, not a claim that local
        # fixtures resolve the complete native workspace dependency graph.
        rust_step = workflow.split("- name: Run OnGROW Rust tests", 1)[1].split("\n      - name:", 1)[0]
        lines = [line.strip() for line in rust_step.splitlines() if line.strip()]
        commands = [(index, line) for index, line in enumerate(lines)
                    if line.startswith(("cargo ", "python "))]
        self.assertEqual(len(commands), 8)
        self.assertEqual(commands[0][1],
                         "cargo metadata --locked --filter-platform x86_64-pc-windows-msvc --format-version 1 | Out-Null")
        self.assertNotIn("--no-deps", rust_step)
        for index, command in commands:
            self.assertRegex(lines[index + 1], r'^if \(\$LASTEXITCODE -ne 0\) \{ throw "[^"]+" \}$', command)
            if command.startswith("cargo "):
                self.assertIn("--locked", command)
        smoke = workflow.split("- name: Verify actual Setup payload offline", 1)[1].split("\n      - name:", 1)[0]
        self.assertIn('ONGROW_CI_SMOKE_TEST: "1"', smoke)
        self.assertIn("test_ongrow_windows_setup_payload.ps1", smoke)
        self.assertIn("-SourceSha \"$env:SOURCE_SHA\"", smoke)
        self.assertIn("setup-unsigned-lab", workflow)
        self.assertIn("dist/ongrow-support-desk-1.4.9-windows-x64-unsigned-lab/", workflow)
        self.assertNotIn("--silent-install", smoke)
        source = (setup.ROOT / "scripts/test_ongrow_windows_setup_payload.ps1").read_text()
        self.assertIn("--ongrow-verify-payload", source)
        self.assertIn("renamed-download.exe", source)
        self.assertIn("WaitForExit(100)", source)
        self.assertIn("$process.ExitCode -ne 0", source)
        self.assertIn("taskkill /PID $process.Id /T /F", source)
        self.assertNotIn("Remove-Item -Recurse", source)

    def test_isolated_rust_cache_contract(self):
        compiler = os.environ.get("ONGROW_TEST_RUSTC", "rustc")
        binary = self.root / ("cache-tests.exe" if os.name == "nt" else "cache-tests")
        subprocess.run([compiler, "--edition=2021", "--test",
                        str(setup.ROOT / "libs/portable/src/ongrow_setup.rs"), "-o", str(binary)], check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    unittest.main(verbosity=2)
