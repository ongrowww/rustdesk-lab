#!/usr/bin/env python3
"""Portable validation/XML tests. These are not native MSI lifecycle tests."""
import json
import os
import re
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import types
import unittest
from unittest import mock
import xml.etree.ElementTree as ET

import package_ongrow_windows_msi as msi

SHA = "a" * 40
NS = {"w": msi.NS}


def fake_pe(identities=(), dll=False):
    data = bytearray(256)
    data[:2] = b"MZ"
    struct.pack_into("<I", data, 0x3c, 128)
    data[128:132] = b"PE\0\0"
    struct.pack_into("<H", data, 132, 0x8664)
    struct.pack_into("<H", data, 150, 0x2000 if dll else 2)
    for value in identities:
        data.extend((value + "\0").encode("utf-16le"))
    return bytes(data)


def fixture(source, product, probe=False, sequence=1):
    source.mkdir()
    p = msi.profile(product, probe)
    identities = [p["name"], "ONGROW_MSI_PROBE_NO_NETWORK"] if probe else ["OnGROW GmbH", p["name"], p["internal"], p["exe"]]
    (source / p["exe"]).write_bytes(fake_pe(identities))
    if probe:
        (source / "probe-version.txt").write_text(str(sequence))
        return
    (source / "librustdesk.dll").write_bytes(fake_pe(dll=True) + p["name"].encode("ascii"))
    (source / "flutter_windows.dll").write_bytes(fake_pe(dll=True))
    for rel in ("data/icudtl.dat", "data/flutter_assets/AssetManifest.json", "LICENCE"):
        path = source / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(b"fixture")
    (source / "ongrow-build-provenance.txt").write_text(f"source={SHA}\n")


class MsiTests(unittest.TestCase):
    def setUp(self):
        # A real macOS /private temp ancestor is intentionally refused by packaging.
        self.temp = tempfile.TemporaryDirectory(prefix="ongrow-msi-test-", dir=Path.home())
        self.root = Path(self.temp.name).resolve()
        self.source = self.root / "source"
        self.output = self.root / "output"
        fixture(self.source, "customer-desk")

    def tearDown(self):
        self.temp.cleanup()

    def package(self, **kwargs):
        return msi.package(kwargs.get("source", self.source), kwargs.get("output", self.output),
                           kwargs.get("product", "customer-desk"), kwargs.get("sequence", 1),
                           kwargs.get("upstream", "1.4.9"), kwargs.get("sha", SHA), True, kwargs.get("probe", False))

    def reject(self, **kwargs):
        with self.assertRaises(ValueError):
            self.package(**kwargs)
        self.assertFalse(self.output.exists())

    def tree(self, product="customer-desk", sequence=1, probe=False):
        return ET.fromstring(msi.generate(product, sequence, "1.4.9", SHA,
                            [msi.profile(product, probe)["exe"], "data/asset"], probe))

    def test_msi_version_exact_order_boundaries(self):
        values = [1, 65535, 65536, 16777215, 16777216, msi.MAX_SEQUENCE]
        self.assertEqual(msi.version(1), "0.0.1")
        self.assertEqual(msi.version(65536), "0.1.0")
        self.assertEqual(msi.version(msi.MAX_SEQUENCE), "255.255.65535")
        triples = [tuple(map(int, msi.version(v).split("."))) for v in values]
        self.assertEqual(triples, sorted(triples))
        for bad in [0, -1, msi.MAX_SEQUENCE + 1, True, "1"]:
            with self.assertRaises(ValueError): msi.version(bad)

    def test_xml_components_deterministic_upgrade_ids_and_product_codes(self):
        first, second = self.tree(), self.tree(sequence=2)
        p1, p2 = first.find("w:Package", NS), second.find("w:Package", NS)
        self.assertEqual(p1.attrib["UpgradeCode"], p2.attrib["UpgradeCode"])
        self.assertNotEqual(p1.attrib["ProductCode"], p2.attrib["ProductCode"])
        c1 = {e.attrib["Id"]: e.attrib["Guid"] for e in first.findall(".//w:Component", NS)}
        c2 = {e.attrib["Id"]: e.attrib["Guid"] for e in second.findall(".//w:Component", NS)}
        self.assertEqual(c1, c2)
        for e in first.findall(".//w:File", NS): self.assertEqual(e.attrib["KeyPath"], "yes")
        upgrade = first.find(".//w:MajorUpgrade", NS)
        self.assertEqual(upgrade.attrib["Schedule"], "afterInstallInitialize")
        self.assertNotIn("AllowSameVersionUpgrades", upgrade.attrib)
        self.assertNotIn("AllowDowngrades", upgrade.attrib)
        same = first.find(".//w:UpgradeVersion", NS)
        self.assertEqual(same.attrib["Property"], "SAME_VERSION_PRODUCT")
        self.assertEqual(same.attrib["OnlyDetect"], "yes")

    def test_scope_service_console_uri_and_no_custom_actions(self):
        desk, console = self.tree(), self.tree("support-console")
        self.assertEqual(desk.find("w:Package", NS).attrib["Scope"], "perMachine")
        self.assertEqual(console.find("w:Package", NS).attrib["Scope"], "perUser")
        service = desk.find(".//w:ServiceInstall", NS)
        self.assertEqual(service.attrib["Account"], "LocalSystem")
        self.assertEqual(service.attrib["Arguments"], "--service")
        self.assertEqual(service.attrib["Vital"], "yes")
        self.assertEqual(desk.find(".//w:ServiceControl", NS).attrib["Wait"], "yes")
        self.assertIsNone(console.find(".//w:ServiceInstall", NS))
        self.assertIsNone(console.find(".//w:ServiceControl", NS))
        for tree in (desk, console):
            self.assertFalse(tree.findall(".//w:CustomAction", NS))
            self.assertFalse(tree.findall(".//w:RemoveFolderEx", NS))
        values = console.findall(".//w:RegistryValue", NS)
        self.assertTrue(all(v.attrib["Root"] == "HKCU" for v in values))
        self.assertTrue(any(v.attrib["Key"] == r"Software\Classes\ongrow-support-console\shell\open\command" for v in values))
        for comp in console.findall(".//w:Component", NS):
            self.assertTrue(any(v.attrib.get("KeyPath") == "yes" for v in comp.findall("w:RegistryValue", NS)))
        removals = console.findall(".//w:RemoveFolder", NS)
        self.assertTrue(removals)
        self.assertTrue(all(v.attrib["On"] == "uninstall" for v in removals))
        self.assertTrue(all(v.attrib["Directory"] != "LocalAppDataFolder" for v in removals))

    def test_console_custom_profile_directories_have_only_empty_uninstall_cleanup(self):
        for probe in (False, True):
            for sequence in (1, 2):
                files = [msi.profile("support-console", probe)["exe"], "data/nested/asset", "other/asset"]
                tree = ET.fromstring(msi.generate("support-console", sequence, "1.4.9", SHA, files, probe))
                custom_dirs = {d.attrib["Id"] for d in tree.findall(".//w:Directory", NS)}
                self.assertTrue({"ProgramsFolder", "OnGrowFolder", "INSTALLFOLDER"}.issubset(custom_dirs))
                cleanup = tree.find(".//w:Component[@Id='OwnPackageRegistry']", NS)
                removals = cleanup.findall("w:RemoveFolder", NS)
                self.assertEqual({r.attrib["Directory"] for r in removals}, custom_dirs)
                self.assertEqual(len(removals), len(custom_dirs))
                self.assertTrue(all(set(r.attrib) == {"Id", "Directory", "On"} and r.attrib["On"] == "uninstall" for r in removals))
                self.assertNotIn("LocalAppDataFolder", {r.attrib["Directory"] for r in removals})
                self.assertFalse(tree.findall(".//w:RemoveFile", NS))
                self.assertFalse(tree.findall(".//w:RemoveFolderEx", NS))
                self.assertEqual(cleanup.attrib["Guid"], msi.identity("support-console", "package-registry", probe))
                expected_shared = {msi.identifier("r", "shared/" + d): d for d in ("ProgramsFolder", "OnGrowFolder")}
                self.assertEqual({r.attrib["Id"]: r.attrib["Directory"] for r in removals if r.attrib["Directory"] in expected_shared.values()}, expected_shared)
            desk = self.tree("customer-desk", probe=probe)
            self.assertFalse(desk.findall(".//w:RemoveFolder", NS))
            self.assertFalse(desk.findall(".//w:RemoveFile", NS))
            self.assertFalse(desk.findall(".//w:RemoveFolderEx", NS))

    def test_probe_never_shares_identity_paths_service_uri(self):
        for product in msi.PROFILES:
            real, probe = self.tree(product), self.tree(product, probe=True)
            self.assertNotEqual(real.find("w:Package", NS).attrib["UpgradeCode"], probe.find("w:Package", NS).attrib["UpgradeCode"])
            self.assertTrue(set(e.attrib["Guid"] for e in real.findall(".//w:Component", NS)).isdisjoint(e.attrib["Guid"] for e in probe.findall(".//w:Component", NS)))
            text = ET.tostring(probe).decode()
            self.assertNotIn("OnGROW Support Desk", text)
            self.assertNotIn("OnGROW Support Console", text)
            self.assertNotIn("ongrow-support-console", text)
            actions = probe.findall(".//w:CustomAction", NS)
            self.assertEqual(len(actions), 1)
            self.assertEqual(actions[0].attrib["ExeCommand"], "--fail-update")
            self.assertEqual(actions[0].attrib["Execute"], "deferred")
            self.assertEqual(actions[0].attrib["Return"], "check")

    def test_legacy_block_is_ui_and_execute_launch_condition(self):
        tree = self.tree()
        self.assertIsNotNone(tree.find(".//w:DirectorySearch/w:FileSearch", NS))
        condition = next(e.attrib["Condition"] for e in tree.findall(".//w:Launch", NS) if "LEGACY_PAYLOAD" in e.attrib["Condition"])
        self.assertIn("WIX_UPGRADE_DETECTED", condition)
        self.assertIn("NOT LEGACY_PAYLOAD", condition)
        self.assertIn("NOT LEGACY_UNINSTALL", condition)
        self.assertIn("NOT LEGACY_REGISTRATION", condition)
        self.assertFalse(tree.findall(".//w:InstallUISequence", NS))

    def test_payload_staged_exactly_and_completion_last(self):
        self.package()
        descriptor = json.loads((self.output / "package.json").read_text())
        self.assertFalse(descriptor["compiled"])
        self.assertFalse(descriptor["probe_only"])
        for rel in descriptor["payload_sha256"]:
            self.assertEqual((self.output / "payload" / rel).read_bytes(), (self.source / rel).read_bytes())
        with self.assertRaises(ValueError): self.package()

    def test_all_required_and_wrong_architecture_role_provenance_rejected(self):
        for rel in [p.relative_to(self.source) for p in self.source.rglob("*") if p.is_file()]:
            path = self.source / rel; data = path.read_bytes(); path.unlink()
            self.reject(); path.write_bytes(data)
        self.reject(product="support-console")
        self.reject(sha="b" * 40)
        executable = self.source / "OnGROW Support Desk.exe"
        original = executable.read_bytes()
        for invalid in [b"not PE", original.replace(b"\x64\x86", b"\x4c\x01", 1), fake_pe(["OnGROW Support Desk"], True)]:
            executable.write_bytes(invalid); self.reject()
        executable.write_bytes(original)
        (self.source / "flutter_windows.dll").write_bytes(fake_pe(dll=False))
        self.reject()

    def test_sensitive_foreign_files_and_paths_fail_before_write(self):
        for rel in ["other.exe", "data/private/test", "data/config/test", "data/.env.example", "logs/a", "key.pem", "config.toml"]:
            path = self.source / rel; path.parent.mkdir(parents=True, exist_ok=True); path.write_bytes(b"refused")
            self.reject(); path.unlink()
            if path.parent != self.source and not any(path.parent.iterdir()): path.parent.rmdir()
        for bad in ["../x", "NUL", "COM1.txt", "a.", "a ", "a:stream", "a/../b", "\\\\host\\x"]:
            with self.assertRaises(ValueError): msi.relative_path(bad)

    def test_links_overlap_existing_output(self):
        original = self.source / "LICENCE"
        hard = self.source / "hard"
        os.link(original, hard)
        self.reject(); hard.unlink()
        try:
            link = self.source / "link"; link.symlink_to(original)
        except OSError:
            link = None
        if link is not None: self.reject(); link.unlink()
        self.reject(output=self.source / "nested")
        self.reject(output=self.root)
        self.output.mkdir()
        sentinel = self.output / "keep"; sentinel.write_text("keep")
        with self.assertRaises(ValueError): self.package()
        self.assertEqual(sentinel.read_text(), "keep")

    def test_windows_alias_and_reparse_metadata_in_real_validation(self):
        original = self.source / "LICENCE"
        metadata = original.lstat()
        real_lstat = Path.lstat
        for name in ("CON.txt", "trailing."):
            injected = self.source / name
            def fixture_lstat(path, *args, **kwargs):
                return metadata if path == injected else real_lstat(path, *args, **kwargs)
            with mock.patch.object(msi.os, "walk", return_value=[(str(self.source), [], [name])]), mock.patch.object(Path, "lstat", fixture_lstat):
                self.reject()
        redirected = types.SimpleNamespace(st_mode=metadata.st_mode, st_file_attributes=0x400)
        def reparse_lstat(path, *args, **kwargs):
            return redirected if path == original else real_lstat(path, *args, **kwargs)
        with mock.patch.object(Path, "lstat", reparse_lstat): self.reject()
        # Case-alias directory entries are injected; never create Windows aliases.
        duplicate = self.source / "licence"
        def duplicate_lstat(path, *args, **kwargs):
            return metadata if path == duplicate else real_lstat(path, *args, **kwargs)
        with mock.patch.object(msi.os, "walk", return_value=[(str(self.source), [], ["LICENCE", "licence"])]), mock.patch.object(Path, "lstat", duplicate_lstat):
            self.reject()

    def test_source_change_during_copy_leaves_no_completion(self):
        target = self.source / "LICENCE"
        real_open = Path.open
        raced = []
        class ChangingReader:
            def __init__(self, stream): self.stream = stream
            def __enter__(self): return self
            def __exit__(self, *args): return self.stream.__exit__(*args)
            def fileno(self): return self.stream.fileno()
            def read(self, size):
                chunk = self.stream.read(size)
                if chunk and not raced:
                    raced.append(True)
                    target.write_bytes(b"changed during copy")
                return chunk
        def changing_open(path, *args, **kwargs):
            stream = real_open(path, *args, **kwargs)
            return ChangingReader(stream) if path == target and args == ("rb",) else stream
        with mock.patch.object(Path, "open", changing_open):
            with self.assertRaisesRegex(ValueError, "Source changed"):
                self.package()
        self.assertEqual(raced, [True])
        self.assertFalse((self.output / "package.json").exists())

    def test_compile_failure_retains_only_incomplete_diagnostic_not_completion(self):
        with mock.patch.object(msi.subprocess, "run", side_effect=subprocess.CalledProcessError(1, "dotnet")):
            with self.assertRaises(subprocess.CalledProcessError):
                msi.package(self.source, self.output, "customer-desk", 1, "1.4.9", SHA)
        self.assertTrue((self.output / "Package.wxs").is_file())
        self.assertFalse((self.output / "package.json").exists())

    def test_console_and_probe_payloads(self):
        for product, probe in [("support-console", False), ("customer-desk", True), ("support-console", True)]:
            source = self.root / (product + str(probe)); fixture(source, product, probe)
            output = self.root / ("out-" + product + str(probe))
            self.package(source=source, output=output, product=product, probe=probe)
            self.assertEqual(json.loads((output / "package.json").read_text())["probe_only"], probe)

    def test_invalid_metadata_rejected_before_output(self):
        for kwargs in [dict(sha="A" * 40), dict(upstream="../bad"), dict(sequence=0), dict(product="foreign")]:
            self.reject(**kwargs)

    def test_metadata_diagnostics_expose_only_field_names(self):
        expected = (101, 202, 3, 404, 505)
        actual = (909, 202, 8, 404, 606)
        self.assertEqual(msi.differing_fields(expected, actual), "st_dev,st_size,st_ctime_ns")
        self.assertEqual(msi.differing_fields(expected, expected), "")

    def test_guardian_diagnostic_is_bounded_enum_only_and_phase_aware(self):
        script = (Path(__file__).parent / "test_ongrow_windows_msi_lifecycle.ps1").read_text(encoding="utf-8", errors="strict")
        diagnostic = script.split("function Get-GuardianDiagnostic(", 1)[1].split("function Throw-GuardianFailure(", 1)[0]
        self.assertLess(diagnostic.index("if (-not $Guardian.Process.HasExited)"), diagnostic.index("$Guardian.Errors.Wait(1000)"))
        self.assertIn("$diagnostic.Length -le 256", diagnostic)
        self.assertIn("$diagnostic -cmatch $pattern", diagnostic)
        self.assertIn("\\A(?:ONGROW_GUARDIAN_FAILURE phase=", diagnostic)
        self.assertIn("(\\r?\\n)?\\z", diagnostic)
        self.assertIn("return 'missing-diagnostic'", diagnostic)
        self.assertNotIn("ReadToEnd", diagnostic)
        self.assertNotIn("Write-", diagnostic)
        failure = script.split("function Throw-GuardianFailure(", 1)[1].split("function Read-Guardian(", 1)[0]
        self.assertIn("$Phase -notin @('bootstrap','read','session','transaction'", failure)
        self.assertIn("$Phase = 'unknown'", failure)
        self.assertIn("$Category = 'unknown'", failure)
        self.assertIn("exit=$exit $diagnostic", failure)
        controller = script.split("function Read-Guardian(", 1)[1].split("# Compile production schemas", 1)[0]
        self.assertIn("$read.Wait(20000)", controller)
        self.assertIn("Read-Guardian $guardian $false 'bootstrap'", controller)
        self.assertIn("Read-Guardian $Guardian ($Operation -eq 'close') $Operation", controller)
        self.assertNotIn("throw $_", controller)
        self.assertNotIn("throw $read.Result", controller)
        self.assertNotIn("throw $Guardian.Errors", controller)

    def test_bootstrap_marker_regex_accepts_only_complete_fixed_fields(self):
        from test_ongrow_native_apply import BOOTSTRAP_FIELDS, BootstrapFailure, report_guardian_failure
        import contextlib
        import io
        script = (Path(__file__).parent / "test_ongrow_windows_msi_lifecycle.ps1").read_text(encoding="utf-8", errors="strict")
        # Run the same enum/anchor regex over synthetic inputs. This is a source
        # regression, not native PowerShell execution or MSI proof.
        pattern = re.search(r"\$pattern = '([^']+)'", script).group(1).replace(r"\z", r"\Z")
        diagnostic = {field: max(values, key=len) for field, values in BOOTSTRAP_FIELDS.items()}
        output = io.StringIO()
        with contextlib.redirect_stderr(output):
            report_guardian_failure("bootstrap", BootstrapFailure(diagnostic))
        longest = output.getvalue()
        self.assertLessEqual(len(longest), 256)
        self.assertIsNotNone(re.fullmatch(pattern, longest))
        for field, values in BOOTSTRAP_FIELDS.items():
            for value in values:
                self.assertIsNotNone(re.fullmatch(pattern, longest.replace(field + "=" + diagnostic[field], field + "=" + value)))
        for malformed in (longest + longest, "raw-token " + longest, longest + "raw-token",
                          longest.replace("phase=bootstrap", "phase=read"),
                          longest.replace(" gate=MissingGate", ""),
                          longest.replace(" reject=", " extra=unknown reject="),
                          longest.replace(" owner=" + diagnostic["owner"], " owner=synthetic-token")):
            self.assertIsNone(re.fullmatch(pattern, malformed))

    def test_native_optional_queries_keep_outer_array_and_preinstall_regression(self):
        script = Path(__file__).with_name("test_ongrow_windows_msi_lifecycle.ps1").read_text()
        for variable, table in (("actions", "CustomAction"), ("services", "ServiceInstall")):
            assignment = next(line.strip() for line in script.splitlines() if line.strip().startswith(f"${variable} ="))
            self.assertTrue(assignment.startswith(f"${variable} = @(if ($tableNames -contains '{table}') {{ Read-Rows "))
            self.assertTrue(assignment.endswith(" })"))
            self.assertIn(f"${variable}.Count", script)
        self.assertIn("foreach ($rowCount in @(0,1,3))", script)
        self.assertIn("foreach ($tablePresent in @($false,$true))", script)
        self.assertIn("$rows = @(if ($tablePresent) { Read-Rows", script)
        self.assertLess(script.index("\nAssert-ReadRowsContract\n"), script.index("# Compile production schemas"))
        self.assertIn("finally { $null = $view.Close() }", script)


if __name__ == "__main__":
    print("MSI unittest Python version: " + ".".join(map(str, sys.version_info[:3])), flush=True)
    unittest.main()
