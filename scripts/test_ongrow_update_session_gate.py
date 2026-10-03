#!/usr/bin/env python3
"""Compile the actual gate alone, then run source-boundary and native OS tests."""
import os
import json
import ctypes
from pathlib import Path, PureWindowsPath
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "scripts/fixtures/ongrow_session_gate_probe"


class KnownFolderGuid(ctypes.Structure):
    _fields_ = [("data1", ctypes.c_uint32), ("data2", ctypes.c_uint16),
                ("data3", ctypes.c_uint16), ("data4", ctypes.c_uint8 * 8)]


# Public SDK FOLDERID_LocalAppData, not a machine or user identifier.
LOCAL_APP_DATA_ID = KnownFolderGuid(0xF1B32785, 0x6FBA, 0x4FCF,
                                   (ctypes.c_uint8 * 8)(0x9D, 0x55, 0x7B, 0x8E, 0x7F, 0x15, 0x70, 0x91))


def windows_probe_state_directory(functions=None, create=None):
    """Resolve and validate the OS parent before creating any probe state."""
    if os.name != "nt" or os.environ.get("GITHUB_ACTIONS") != "true":
        raise RuntimeError("windows-probe-runner-required")
    if functions is None:
        try:
            # Load only system DLLs, only on the disposable Windows runner.
            shell = ctypes.WinDLL("shell32.dll", winmode=0x800)
            ole = ctypes.WinDLL("ole32.dll", winmode=0x800)
            kernel = ctypes.WinDLL("kernel32.dll", winmode=0x800)
            functions = (shell.SHGetKnownFolderPath, ole.CoTaskMemFree,
                         kernel.GetDriveTypeW, kernel.GetFileAttributesW)
        except (OSError, AttributeError):
            raise RuntimeError("windows-probe-api-unavailable") from None
    known_folder, free, drive_type, attributes = functions
    known_folder.argtypes = [ctypes.POINTER(KnownFolderGuid), ctypes.c_uint32,
                             ctypes.c_void_p, ctypes.POINTER(ctypes.c_void_p)]
    known_folder.restype = ctypes.c_int32  # HRESULT is signed and always 32-bit.
    free.argtypes, free.restype = [ctypes.c_void_p], None
    drive_type.argtypes, drive_type.restype = [ctypes.c_wchar_p], ctypes.c_uint32
    attributes.argtypes, attributes.restype = [ctypes.c_wchar_p], ctypes.c_uint32
    allocation = ctypes.c_void_p()
    try:
        try:
            # KF_FLAG_DEFAULT and NULL token select the current process user.
            result = known_folder(ctypes.byref(LOCAL_APP_DATA_ID), 0, None,
                                  ctypes.byref(allocation))
            if result != 0:
                raise RuntimeError("windows-probe-known-folder-error")
            if not allocation.value:
                raise RuntimeError("windows-probe-known-folder-null")
            raw_parent = ctypes.wstring_at(allocation.value)
        except (OSError, ValueError):
            raise RuntimeError("windows-probe-known-folder-error") from None
    finally:
        # The SDK requires freeing the out pointer even after a failed HRESULT.
        free(allocation)

    parent = PureWindowsPath(raw_parent)
    drive = parent.drive
    if (not parent.is_absolute() or len(drive) != 2
            or drive[0] not in "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"
            or drive[1] != ":" or parent.root != "\\"):
        raise RuntimeError("windows-probe-parent-not-local")
    # Reject raw components before PureWindowsPath can normalize them away.
    components = raw_parent[3:].split("\\")
    if (raw_parent[:3] != parent.anchor or not components
            or any(not part or part in [".", ".."] or part.endswith((".", " "))
                   or any(char in part for char in '/<>:"|?*') for part in components)):
        raise RuntimeError("windows-probe-parent-not-normal")
    try:
        if drive_type(parent.anchor) != 3:  # DRIVE_FIXED only.
            raise RuntimeError("windows-probe-drive-not-fixed")
    except OSError:
        raise RuntimeError("windows-probe-drive-error") from None
    prefix = PureWindowsPath(parent.anchor)
    for component in [None] + components:
        if component is not None:
            prefix /= component
        try:
            flags = attributes(str(prefix))
        except OSError:
            raise RuntimeError("windows-probe-attributes-error") from None
        if flags == 0xFFFFFFFF:
            raise RuntimeError("windows-probe-attributes-error")
        if flags & 0x400:  # FILE_ATTRIBUTE_REPARSE_POINT.
            raise RuntimeError("windows-probe-parent-reparse")
        if not flags & 0x10:  # Every existing component must be a directory.
            raise RuntimeError("windows-probe-parent-not-directory")
    try:
        factory = create if create is not None else tempfile.TemporaryDirectory
        return factory(prefix="ongrow-gate-state-", dir=str(parent))
    except OSError:
        raise RuntimeError("windows-probe-state-create-error") from None


class FakeWindowsProbeApi:
    """No OS calls or writes; only synthetic strings and allocated test memory."""
    def __init__(self, path=r"C:\probe\Local", result=0, drive=3, attributes=None):
        self.buffer = ctypes.create_unicode_buffer(path) if path is not None else None
        self.result = result
        self.known_folder = mock.Mock(side_effect=self.get_folder)
        self.free = mock.Mock()
        self.drive_type = mock.Mock(return_value=drive)
        self.attributes = mock.Mock(return_value=0x10, side_effect=attributes)
        self.directory = mock.Mock()
        self.create = mock.Mock(return_value=self.directory)

    def get_folder(self, identifier, flags, token, output):
        if self.buffer is not None:
            ctypes.cast(output, ctypes.POINTER(ctypes.c_void_p))[0] = ctypes.addressof(self.buffer)
        return self.result

    def functions(self):
        return self.known_folder, self.free, self.drive_type, self.attributes


class WindowsProbePlacementTests(unittest.TestCase):
    def invoke(self, api, platform="nt", marker="true"):
        with mock.patch.object(os, "name", platform), mock.patch.dict(os.environ, clear=False):
            if marker is None:
                os.environ.pop("GITHUB_ACTIONS", None)
            else:
                os.environ["GITHUB_ACTIONS"] = marker
            return windows_probe_state_directory(api.functions(), api.create)

    def test_sdk_abi_current_token_free_and_creation_order(self):
        api = FakeWindowsProbeApi()
        self.assertIs(self.invoke(api), api.directory)
        self.assertEqual(ctypes.sizeof(KnownFolderGuid), 16)
        self.assertTrue((LOCAL_APP_DATA_ID.data1, LOCAL_APP_DATA_ID.data2, LOCAL_APP_DATA_ID.data3)
                        == (0xF1B32785, 0x6FBA, 0x4FCF))
        self.assertTrue(bytes(LOCAL_APP_DATA_ID.data4) == bytes.fromhex("9d557b8e7f157091"))
        self.assertEqual(api.known_folder.restype, ctypes.c_int32)
        self.assertEqual(api.known_folder.argtypes, [ctypes.POINTER(KnownFolderGuid), ctypes.c_uint32,
                                                   ctypes.c_void_p, ctypes.POINTER(ctypes.c_void_p)])
        self.assertEqual(api.free.argtypes, [ctypes.c_void_p])
        self.assertIsNone(api.free.restype)
        for function in [api.drive_type, api.attributes]:
            self.assertEqual(function.argtypes, [ctypes.c_wchar_p])
            self.assertEqual(function.restype, ctypes.c_uint32)
        self.assertEqual(api.known_folder.call_args.args[1:3], (0, None))
        self.assertIs(api.known_folder.call_args.args[0]._obj, LOCAL_APP_DATA_ID)
        self.assertEqual(api.free.call_count, 1)
        self.assertTrue(api.free.call_args.args[0].value == ctypes.addressof(api.buffer))
        self.assertEqual(api.drive_type.call_args.args, ("C:\\",))
        self.assertEqual(api.attributes.call_args_list, [mock.call("C:\\"), mock.call(r"C:\probe"), mock.call(r"C:\probe\Local")])
        api.create.assert_called_once_with(prefix="ongrow-gate-state-", dir=r"C:\probe\Local")
        # One shared call log proves validation and freeing precede the write.
        api = FakeWindowsProbeApi()
        calls = mock.Mock()
        for name in ["known_folder", "free", "drive_type", "attributes", "create"]:
            calls.attach_mock(getattr(api, name), name)
        self.invoke(api)
        self.assertEqual([call[0] for call in calls.mock_calls],
                         ["known_folder", "free", "drive_type", "attributes", "attributes", "attributes", "create"])

    def test_known_folder_error_null_and_api_exception_free_before_writes(self):
        for case in ["hresult", "null", "exception"]:
            with self.subTest(case=case):
                api = FakeWindowsProbeApi(path=None if case == "null" else r"C:\probe\Local",
                                          result=-2147467259 if case == "hresult" else 0)
                if case == "exception":
                    api.known_folder.side_effect = OSError("opaque test error")
                expected = "windows-probe-known-folder-null" if case == "null" else "windows-probe-known-folder-error"
                with self.assertRaisesRegex(RuntimeError, "^" + expected + "$"):
                    self.invoke(api)
                api.free.assert_called_once()
                expected_pointer = ctypes.addressof(api.buffer) if case == "hresult" else None
                self.assertTrue(api.free.call_args.args[0].value == expected_pointer)
                api.drive_type.assert_not_called()
                api.attributes.assert_not_called()
                api.create.assert_not_called()

    def test_nonlocal_and_non_normal_paths_fail_before_attributes_or_writes(self):
        cases = [(r"probe\Local", "not-local"), (r"C:probe\Local", "not-local"),
                 (r"\probe\Local", "not-local"), (r"\\server\share\Local", "not-local"),
                 (r"\\?\C:\probe\Local", "not-local"), (r"C:\probe\..\Local", "not-normal"),
                 (r"C:\probe\.\Local", "not-normal"), ("C:/probe/Local", "not-normal"),
                 (r"C:\probe:stream\Local", "not-normal"), ("C:\\", "not-normal")]
        for path, category in cases:
            with self.subTest(category=category):
                api = FakeWindowsProbeApi(path=path)
                with self.assertRaisesRegex(RuntimeError, "^windows-probe-parent-" + category + "$"):
                    self.invoke(api)
                api.free.assert_called_once()
                api.drive_type.assert_not_called()
                api.attributes.assert_not_called()
                api.create.assert_not_called()

    def test_nonfixed_disk_or_drive_api_error_fails_before_attributes_or_writes(self):
        for kind in [0, 1, 2, 4, 5, 6, "exception"]:
            with self.subTest(kind=kind):
                api = FakeWindowsProbeApi(drive=kind)
                if kind == "exception":
                    api.drive_type.side_effect = OSError("opaque test error")
                category = "error" if kind == "exception" else "not-fixed"
                with self.assertRaisesRegex(RuntimeError, "^windows-probe-drive-" + category + "$"):
                    self.invoke(api)
                api.free.assert_called_once()
                api.attributes.assert_not_called()
                api.create.assert_not_called()

    def test_each_existing_component_rejects_reparse_invalid_or_non_directory(self):
        for index in range(3):
            for flags, category in [(0x410, "parent-reparse"), (0xFFFFFFFF, "attributes-error"),
                                    (0x80, "parent-not-directory"), (OSError("opaque test error"), "attributes-error")]:
                with self.subTest(component=index, category=category):
                    api = FakeWindowsProbeApi(attributes=[0x10] * index + [flags])
                    with self.assertRaisesRegex(RuntimeError, "^windows-probe-" + category + "$"):
                        self.invoke(api)
                    api.free.assert_called_once()
                    self.assertEqual(api.attributes.call_count, index + 1)
                    api.create.assert_not_called()

    def test_runner_and_windows_required_before_loading_or_writing(self):
        for platform, marker in [("posix", "true"), ("nt", None), ("nt", "false"), ("nt", "1")]:
            with self.subTest(platform=platform, marker=marker):
                api = FakeWindowsProbeApi()
                with self.assertRaisesRegex(RuntimeError, "^windows-probe-runner-required$"):
                    self.invoke(api, platform, marker)
                api.known_folder.assert_not_called()
                api.free.assert_not_called()
                api.create.assert_not_called()
        with mock.patch.object(os, "name", "posix"), mock.patch.object(ctypes, "WinDLL", create=True) as load:
            with self.assertRaisesRegex(RuntimeError, "^windows-probe-runner-required$"):
                windows_probe_state_directory()
            load.assert_not_called()

    def test_creation_error_has_only_a_fixed_category(self):
        api = FakeWindowsProbeApi()
        api.create.side_effect = OSError("opaque test error")
        with self.assertRaisesRegex(RuntimeError, "^windows-probe-state-create-error$"):
            self.invoke(api)
        api.free.assert_called_once()
        api.create.assert_called_once()

    def test_dll_api_load_failure_is_opaque_and_precedes_writes(self):
        with mock.patch.object(os, "name", "nt"), mock.patch.dict(os.environ, {"GITHUB_ACTIONS": "true"}), \
                mock.patch.object(ctypes, "WinDLL", create=True, side_effect=OSError("opaque test error")) as load, \
                mock.patch.object(tempfile, "TemporaryDirectory") as create:
            with self.assertRaisesRegex(RuntimeError, "^windows-probe-api-unavailable$"):
                windows_probe_state_directory()
            load.assert_called_once_with("shell32.dll", winmode=0x800)
            create.assert_not_called()

    def test_native_setup_api_failure_precedes_all_scratch_and_state_writes(self):
        api = FakeWindowsProbeApi(result=-2147467259)
        real_factory = windows_probe_state_directory
        native = SessionGateTests.test_real_native_gate
        native = getattr(native, "__wrapped__", native)  # Also portable when native tests are skipped.
        with mock.patch.object(os, "name", "nt"), mock.patch.dict(os.environ, {"GITHUB_ACTIONS": "true"}), \
                mock.patch(__name__ + ".windows_probe_state_directory",
                           side_effect=lambda: real_factory(api.functions(), api.create)), \
                mock.patch.object(Path, "mkdir") as mkdir, \
                mock.patch.object(tempfile, "TemporaryDirectory") as scratch:
            with self.assertRaisesRegex(RuntimeError, "^windows-probe-known-folder-error$"):
                native(SessionGateTests("test_real_native_gate"))
            api.free.assert_called_once()
            api.create.assert_not_called()
            mkdir.assert_not_called()
            scratch.assert_not_called()


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
        self.assertNotIn("LookupAccountName", windows)
        self.assertNotIn("LookupAccountSid", windows)
        self.assertNotIn("LocalSid", diagnostic)
        self.assertNotIn("lookup-unavailable", diagnostic)
        self.assertIn("fn trusted_installer() -> Option<AllocatedSid>", windows)
        installer = windows.split("fn trusted_installer()", 1)[1].split("\nfn trusted_sid(", 1)[0]
        self.assertIn("IsValidSid(value.0)", installer)
        self.assertIn(".ok()?", installer)
        self.assertIn("return None", installer)
        self.assertEqual(windows.count('w!("S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464")'), 1)
        self.assertIn('ConvertStringSidToSidW(w!("S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464"), &mut value.0)', windows)
        self.assertIn('ConvertStringSidToSidW(w!("S-1-5-80-0"), &mut value.0)', diagnostic)
        self.assertIn("IsValidSid(value.0)", diagnostic)
        self.assertIn("impl Drop for AllocatedSid", windows)
        self.assertIn("LocalFree(Some(HLOCAL(self.0.0)))", windows)
        self.assertIn('ConvertStringSidToSidW(w!("S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478465"), &mut other_service.0)', diagnostic)
        self.assertIn('(other_service.0, "other")', diagnostic)
        self.assertIn("for product in [Product::CustomerDesk, Product::SupportConsole]", diagnostic)
        self.assertIn("for ancestor in [false, true]", diagnostic)
        self.assertIn("fn diagnostic_categories_follow_ancestor_only_service_trust", diagnostic)
        self.assertIn("assert!(trusted_sid(installer.0, &user, product, true))", diagnostic)
        self.assertIn("assert!(!trusted_sid(installer.0, &user, product, false))", diagnostic)
        self.assertIn("assert!(!trusted_sid(sid, &user, product, ancestor))", diagnostic)
        self.assertIn('matches!(category, "system" | "admins")', diagnostic)
        self.assertIn("privileged_user || product == Product::SupportConsole", diagnostic)
        self.assertIn("assert!(!trusted_sid(PSID::default(), &user, product, ancestor))", diagnostic)
        self.assertIn("inspect(&file, user, product, true, index + 1 < components.len())", windows)
        for file in ["gate", "journal"]:
            self.assertIn(f"inspect(&{file}, &user, product, false, false)", windows)
        self.assertIn('if !ancestor && product == Product::SupportConsole && unsafe { EqualSid(owner, user.sid()) }.is_err()', windows)
        self.assertIn('let mutation = if ancestor { 0x000d_0150u32 | 0x5000_0000 } else { 0x000d_0156u32 | 0x5000_0000 };', windows)
        self.assertIn('if ace.Mask & mutation != 0 && !trusted_ace_sid(sid, owner, user, product, ancestor)', windows)
        self.assertIn('"root-volume"', windows)
        trusted = windows.split("fn trusted_sid(", 1)[1].split("\n}\n", 1)[0]
        self.assertEqual(trusted, '''sid: PSID, user: &User, product: Product, ancestor: bool) -> bool {
    if sid.0.is_null() { return false; }
    unsafe {
        IsWellKnownSid(sid, WinLocalSystemSid).as_bool()
            || IsWellKnownSid(sid, WinBuiltinAdministratorsSid).as_bool()
            || (product == Product::SupportConsole && EqualSid(sid, user.sid()).is_ok())
            || (ancestor && trusted_installer().map(|installer| EqualSid(sid, installer.0).is_ok()).unwrap_or(false))
    }''')

    def test_acl_diagnostics_are_fixed_probe_only_and_pure_categories_run(self):
        windows = (ROOT / "src/ongrow_update/session_gate/windows.rs").read_text()
        diagnostic = windows.split("mod owner_diagnostics {", 1)[1].split("\nfn inspect(", 1)[0]
        self.assertIn("#[cfg(all(test, ongrow_session_gate_probe))]\nmod owner_diagnostics {", windows)
        self.assertIn('''if ace.Mask & mutation != 0 && !trusted_ace_sid(sid, owner, user, product, ancestor) {
            #[cfg(all(test, ongrow_session_gate_probe))]
            owner_diagnostics::rejected_access(sid, user, ace.Mask & mutation);
            return Err(untrusted("forbidden-access"));
        }''', windows)
        access = diagnostic.split("    pub(super) fn rejected_access(", 1)[1].split("\n    #[test]", 1)[0]
        self.assertEqual(access, '''sid: PSID, user: &User, mask: u32) {
        eprintln!("ONGROW_GATE_ACCESS_PRINCIPAL:{}", classify(sid, user));
        for category in access_categories(mask) {
            eprintln!("ONGROW_GATE_ACCESS_RIGHT:{category}");
        }
    }
''')
        helper = diagnostic[diagnostic.index("    fn access_categories("):diagnostic.index("    pub(super) fn rejected_access(")]
        pure_test = diagnostic[diagnostic.index("    #[test]\n    fn diagnostic_access_categories_are_fixed_and_masked("):
                               diagnostic.index("    #[test]\n    fn diagnostic_categories_follow_ancestor_only_service_trust(")]
        # Compile the exact std-only helper and its native Rust test on either OS.
        # No Windows API, gate initialization, desktop build or dependency lookup.
        with tempfile.TemporaryDirectory(prefix="ongrow-acl-categories-") as directory:
            source = Path(directory) / "categories.rs"
            binary = Path(directory) / ("categories.exe" if os.name == "nt" else "categories")
            source.write_text(helper + pure_test)
            subprocess.run(["rustc", "--edition=2021", "--test", str(source), "-o", str(binary)],
                           check=True, capture_output=True, text=True, timeout=180)
            result = subprocess.run([str(binary)], check=True, capture_output=True, text=True, timeout=180)
            self.assertIn("1 passed; 0 failed", result.stdout)

    def test_principal_structure_and_positions_are_probe_only_and_run(self):
        windows = (ROOT / "src/ongrow_update/session_gate/windows.rs").read_text()
        before, diagnostic = windows.split("#[cfg(all(test, ongrow_session_gate_probe))]\nmod owner_diagnostics {", 1)
        diagnostic, after = diagnostic.split("\nfn inspect(", 1)
        classes = [("WinAuthenticatedUserSid", "authenticated-users"),
                   ("WinCreatorOwnerRightsSid", "owner-rights"),
                   ("WinBuiltinGuestsSid", "builtin-guests"),
                   ("WinBuiltinPowerUsersSid", "builtin-power-users"),
                   ("WinBuiltinBackupOperatorsSid", "builtin-backup-operators"),
                   ("WinBuiltinRemoteDesktopUsersSid", "builtin-remote-desktop-users"),
                   ("WinBuiltinRemoteManagementUsersSid", "builtin-remote-management-users")]
        for sdk, category in classes:
            self.assertEqual(diagnostic.count(f'({sdk}, "{category}")'), 2)
        for symbol in [sdk for sdk, _ in classes if sdk != "WinCreatorOwnerRightsSid"] + ["GetSidIdentifierAuthority", "GetSidSubAuthorityCount",
                                                   "GetSidSubAuthority", "windows_account_form", "account_form_values"]:
            self.assertNotIn(symbol, before + after)
        classifier = diagnostic.split("    fn classify(", 1)[1].split("    pub(super) fn rejected(", 1)[0]
        self.assertIn("IsWellKnownSid(sid, kind)", classifier)
        self.assertLess(classifier.index("EqualSid(sid, user.sid())"), classifier.index("windows_account_form(sid)"))
        self.assertLess(classifier.index("EqualSid(sid, services.0)"), classifier.index("windows_account_form(sid)"))
        self.assertLess(classifier.index("EqualSid(sid, installer.0)"), classifier.index("windows_account_form(sid)"))
        wrapper = diagnostic[diagnostic.index("    fn windows_account_form("):diagnostic.index("    fn classify(")]
        self.assertEqual(wrapper, '''    fn windows_account_form(sid: PSID) -> bool {
        // Structural category only. Never inspect domain values or the account RID.
        if sid.0.is_null() || !unsafe { IsValidSid(sid) }.as_bool() { return false; }
        let authority = unsafe { GetSidIdentifierAuthority(sid) };
        if authority.is_null() { return false; }
        let authority = unsafe { (*authority).Value };
        if authority != [0, 0, 0, 0, 0, 5] { return false; }
        if sid.0.is_null() || !unsafe { IsValidSid(sid) }.as_bool() { return false; }
        let count = unsafe { GetSidSubAuthorityCount(sid) };
        if count.is_null() { return false; }
        let count = unsafe { *count };
        if count != 5 { return false; }
        if sid.0.is_null() || !unsafe { IsValidSid(sid) }.as_bool() { return false; }
        let first = unsafe { GetSidSubAuthority(sid, 0) };
        if first.is_null() { return false; }
        account_form_values(authority, count, unsafe { *first })
    }
''')
        self.assertIn('ConvertStringSidToSidW(value, &mut sid.0)', diagnostic)
        self.assertIn('assert!(!windows_account_form(PSID::default()))', diagnostic)
        self.assertIn('(w!("S-1-5-21-1-2-3-4"), true)', diagnostic)
        for value in ["S-1-6-21-1-2-3-4", "S-1-5-21-1-2-3", "S-1-5-21-1-2-3-4-5", "S-1-5-20-1-2-3-4", "S-1-0-0"]:
            self.assertIn(f'(w!("{value}"), false)', diagnostic)
        context = after.split('eprintln!("ONGROW_GATE_REJECT_CONTEXT:{}", ', 1)[1].split(");", 1)[0]
        self.assertEqual(context, 'if index == 1 { "root-volume" } else if index + 1 == components.len() { "protected-root" } '
                                 'else if index + 2 == components.len() { "direct-parent" } else { "outer-ancestor" }')
        self.assertIn('#[cfg(all(test, ongrow_session_gate_probe))]\n            eprintln!("ONGROW_GATE_REJECT_CONTEXT:{}"', after)
        helper = diagnostic[diagnostic.index("    fn account_form_values("):diagnostic.index("    fn windows_account_form(")]
        pure_test = diagnostic[diagnostic.index("    #[test]\n    fn diagnostic_account_form_values_are_exact("):
                               diagnostic.index("    #[test]\n    fn diagnostic_account_form_uses_only_public_structure(")]
        context_test = '''
fn context(index: usize, components: &[()]) -> &'static str { ''' + context + ''' }
#[test]
fn positions_are_fixed_categories() {
    for count in 2..=10 {
        let components = vec![(); count];
        assert_eq!(context(1, &components), "root-volume");
        if count > 2 { assert_eq!(context(count - 1, &components), "protected-root"); }
        if count > 3 { assert_eq!(context(count - 2, &components), "direct-parent"); }
        for index in 2..count.saturating_sub(2) {
            assert_eq!(context(index, &components), "outer-ancestor");
        }
    }
}
'''
        # Execute the original std-only helper/test and original context expression.
        # The Windows SID API wrapper remains covered by the native SDK tests.
        with tempfile.TemporaryDirectory(prefix="ongrow-principal-categories-") as directory:
            source = Path(directory) / "categories.rs"
            binary = Path(directory) / ("categories.exe" if os.name == "nt" else "categories")
            source.write_text(helper + pure_test + context_test)
            subprocess.run(["rustc", "--edition=2021", "--test", str(source), "-o", str(binary)],
                           check=True, capture_output=True, text=True, timeout=180)
            result = subprocess.run([str(binary)], check=True, capture_output=True, text=True, timeout=180)
            self.assertIn("2 passed; 0 failed", result.stdout)

    def test_owner_rights_allow_ace_uses_the_same_descriptor_owner(self):
        windows = (ROOT / "src/ongrow_update/session_gate/windows.rs").read_text()
        helper = windows.split("fn trusted_ace_sid(", 1)[1].split("\n// Read-only diagnostics.", 1)[0]
        self.assertEqual(helper, '''sid: PSID, owner: PSID, user: &User, product: Product, ancestor: bool) -> bool {
    if sid.0.is_null() || owner.0.is_null() { return false; }
    if !unsafe { IsValidSid(sid) }.as_bool() || !unsafe { IsValidSid(owner) }.as_bool() { return false; }
    if trusted_sid(sid, user, product, ancestor) { return true; }
    // Owner Rights is object-bound, not a globally trusted principal. The owner
    // comes from the same security descriptor as this ACE, never from a token group.
    unsafe {
        IsWellKnownSid(sid, WinCreatorOwnerRightsSid).as_bool()
            && trusted_sid(owner, user, product, ancestor)
            && (ancestor || product != Product::SupportConsole || EqualSid(owner, user.sid()).is_ok())
    }
}
''')
        inspect = windows.split("fn inspect(", 1)[1].split("\nfn open(", 1)[0]
        self.assertIn('''GetSecurityInfo(handle(file), SE_FILE_OBJECT, OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            Some(&mut owner), None, Some(&mut dacl), None, Some(&mut descriptor.0))''', inspect)
        owner_check = inspect.index("if !trusted_sid(owner, user, product, ancestor)")
        exact_check = inspect.index("if !ancestor && product == Product::SupportConsole")
        acl_loop = inspect.index("for index in 0..size.AceCount")
        ace_check = inspect.index("if ace.Mask & mutation != 0 && !trusted_ace_sid(sid, owner, user, product, ancestor)")
        self.assertLess(owner_check, exact_check)
        self.assertLess(exact_check, acl_loop)
        self.assertLess(acl_loop, ace_check)
        self.assertEqual(inspect.count("trusted_ace_sid("), 1)
        self.assertEqual(inspect.count("GetSecurityInfo("), 1)
        self.assertIn('return Err(untrusted("forbidden-access"));', inspect[ace_check:])
        native = windows.split("mod owner_diagnostics {", 1)[1].split("\nfn inspect(", 1)[0]
        self.assertIn("fn owner_rights_ace_is_bound_to_the_same_trusted_owner", native)
        self.assertIn("CreateWellKnownSid(kind, None, Some(sid), &mut length)", native)
        self.assertIn('ConvertStringSidToSidW(w!("S-1-3-5"), &mut near_rights.0)', native)
        self.assertIn("assert!(!trusted_sid(rights, &user, product, ancestor))", native)
        self.assertIn("let mut invalid_buffer = [0usize; 16]", native)
        self.assertIn("assert!(!unsafe { IsValidSid(invalid) }.as_bool())", native)
        for sid, owner in [("invalid", "system"), ("rights", "invalid"), ("system", "invalid")]:
            self.assertIn(f"assert!(!trusted_ace_sid({sid}, {owner}, &user, product, ancestor))", native)
        self.assertIn("trusted_ace_sid(rights, installer.0, &user, product, ancestor), ancestor", native)
        self.assertIn("for owner in [foreign.0, world, services.0, rights, creator, null_sid, PSID::default()]", native)
        self.assertIn("for sid in [foreign.0, world, services.0, creator, near_rights.0, null_sid, PSID::default()]", native)
        self.assertIn("assert!(![rights, foreign.0].into_iter()", native)
        self.assertIn(".all(|sid| trusted_ace_sid(sid, owner, &user, product, ancestor))", native)
        self.assertIn('''assert_eq!(trusted_ace_sid(sid, system, &user, product, ancestor),
                        trusted_sid(sid, &user, product, ancestor));''', native)

    @unittest.skipUnless(sys.platform in ["darwin", "win32"], "native gate requires macOS or Windows")
    def test_real_native_gate(self):
        platform = "windows" if os.name == "nt" else "macos"
        # Resolve and validate the state parent before any scratch/state writes.
        state_directory = windows_probe_state_directory() if platform == "windows" else None
        if state_directory is not None:
            # Also clean our unique directory if scratch setup fails before try.
            self.addCleanup(state_directory.cleanup)
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
                state = Path(state_directory.name)
            else:
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
