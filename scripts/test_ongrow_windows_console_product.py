#!/usr/bin/env python3
"""Static Windows console product and profile checks, without Windows state."""

import tempfile
import unittest
from pathlib import Path

from apply_ongrow_windows_console_product import REPLACEMENTS, apply

ROOT = Path(__file__).resolve().parents[1]


class WindowsConsoleProductTests(unittest.TestCase):
    def read(self, relative: str) -> str:
        return (ROOT / relative).read_text(encoding="utf-8")

    def test_profile_has_independent_executable_and_resources(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for relative in REPLACEMENTS:
                target = root / relative
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_text(self.read(str(relative)), encoding="utf-8")
            apply(root)
            self.assertIn('set(BINARY_NAME "ongrow_support_console")',
                          (root / "flutter/windows/CMakeLists.txt").read_text())
            self.assertIn('OUTPUT_NAME "OnGROW Support Console"',
                          (root / "flutter/windows/runner/CMakeLists.txt").read_text())
            resources = (root / "flutter/windows/runner/Runner.rc").read_text()
            self.assertIn('"OriginalFilename", "OnGROW Support Console.exe"', resources)
            self.assertIn('"InternalName", "ongrow_support_console"', resources)
            self.assertNotIn('"OnGROW Support Desk.exe"', resources)

    def test_profile_fails_before_any_write_on_missing_anchor(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for relative in REPLACEMENTS:
                target = root / relative
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_text(self.read(str(relative)), encoding="utf-8")
            broken = root / "flutter/windows/runner/main.cpp"
            broken.write_text("missing anchor", encoding="utf-8")
            original = (root / "flutter/windows/CMakeLists.txt").read_text()
            with self.assertRaises(ValueError):
                apply(root)
            self.assertEqual(original, (root / "flutter/windows/CMakeLists.txt").read_text())

    def test_operator_and_runner_preserve_exact_uri(self) -> None:
        runner = self.read("flutter/windows/runner/main.cpp")
        common = self.read("flutter/lib/common.dart")
        operator = self.read("src/ongrow_operator.rs")
        self.assertNotIn("find_last_not_of", runner)
        self.assertIn("DispatchToUniLinksDesktop(hwnd)", runner)
        self.assertIn("mainHandleOngrowOperatorUriSync(uri: rawUri)", common)
        self.assertIn('uri.query().is_some()', operator)
        self.assertIn('"launch" if valid_ticket(value)', operator)

    def test_console_never_starts_portable_incoming_service(self) -> None:
        common = self.read("src/common.rs")
        core = self.read("src/core_main.rs")
        self.assertIn('settings.insert("conn-type".to_owned(), "outgoing".to_owned())', common)
        self.assertIn('settings.insert("disable-installation".to_owned(), "Y".to_owned())', common)
        self.assertIn('_is_quick_support |= crate::get_app_name() != "OnGROW Support Console"', core)

    def test_install_owns_only_per_user_console_identity(self) -> None:
        install = self.read("scripts/install_ongrow_windows_console.ps1")
        uninstall = self.read("scripts/uninstall_ongrow_windows_console.ps1")
        for source in (install, uninstall):
            self.assertIn('HKCU:\\Software\\Classes\\ongrow-support-console', source)
            self.assertIn('OnGROWSupportConsole', source)
            self.assertIn('de.ongrow.supportconsole', source)
            self.assertNotIn('HKEY_CLASSES_ROOT', source)
            self.assertNotIn('OnGROW Support Desk.exe', source)
            self.assertNotIn('RustDesk.exe', source)
        self.assertIn('"{0}" "%1"', install)
        self.assertIn('Registry entry is not owned by Support Console', install)
        self.assertIn('URI association is not owned by this Support Console installation', uninstall)

    def test_workflow_keeps_console_gates_and_artifact_separate(self) -> None:
        workflow = self.read(".github/workflows/ongrow-support-console-windows-x64.yml")
        desk = self.read(".github/workflows/ongrow-lab-windows-x64.yml")
        self.assertIn('group: ongrow-support-console-windows-x64-lab', workflow)
        self.assertIn('--role support-console', workflow)
        self.assertIn('cargo test --locked --lib --features flutter ongrow_operator', workflow)
        self.assertIn('flutter test --no-pub test/ongrow_support_console_test.dart', workflow)
        self.assertIn('Smoke-test initialized Console UI', workflow)
        self.assertIn('Copy-Item scripts/install_ongrow_windows_console.ps1', workflow)
        self.assertIn('if (-not (Test-Path (Join-Path $release "data")', workflow)
        self.assertIn('group: ongrow-support-desk-windows-x64-lab', desk)
        self.assertNotIn('--role support-console', desk)

    def test_ack_is_native_authenticated_and_add_failure_stops_start(self) -> None:
        ffi = self.read("src/flutter_ffi.rs")
        native = self.read("src/flutter.rs")
        model = self.read("flutter/lib/models/model.dart")
        self.assertNotIn("operator_session_ready_sync", ffi)
        self.assertNotIn("operatorSessionReadySync", model)
        transport = native.split("fn set_connection_type(", 1)[1].split("fn set_fingerprint", 1)[0]
        self.assertNotIn("confirm_session", transport)
        connected = native.split("fn on_connected(", 1)[1].split("fn on_login_error", 1)[0]
        self.assertIn("confirm_session_authenticated", connected)
        interface = self.read("src/ui_session_interface.rs")
        error = interface.split("fn handle_login_error(&self, err: &str)", 1)[1].split("fn set_multiple_windows_session", 1)[0]
        self.assertLess(error.index("self.on_login_error()"), error.index("handle_login_error(self.lc"))
        add = model.index("operatorLaunchHandle != null && addRes.isNotEmpty")
        self.assertLess(add, model.index("stream = bind.sessionStart(", add))
        self.assertIn("throw StateError('session_start_failed')", model[add:])

    def test_windows_smoke_proves_rendered_ui_and_runtime_isolation(self) -> None:
        workflow = self.read(".github/workflows/ongrow-support-console-windows-x64.yml")
        self.assertIn('python scripts/test_ongrow_ci_isolation.py', workflow)
        self.assertIn('cargo test --locked --lib --features flutter ongrow_ci', workflow)
        smoke = workflow.split('- name: Smoke-test initialized Console UI', 1)[1].split('- name: Verify and stage', 1)[0]
        self.assertIn('ONGROW_CI_SMOKE_TEST: "1"', smoke)
        self.assertIn("GetProp($process.MainWindowHandle, 'ONGROW_CONSOLE_UI_READY')", smoke)
        self.assertNotIn('UIAutomationClient', smoke)
        runner = self.read('flutter/windows/runner/flutter_window.cpp')
        self.assertIn('call.method_name() == "ongrowConsoleUiReady"', runner)
        self.assertIn('SetNextFrameCallback', runner)
        self.assertIn('ForceRedraw()', runner)
        self.assertIn('::IsWindowVisible(GetHandle())', runner)
        main = self.read('flutter/lib/main.dart')
        self.assertLess(main.index('await WidgetsBinding.instance.endOfFrame'), main.index(".invokeMethod<void>('ongrowConsoleUiReady')"))

    def test_pr_build_checks_out_exact_trusted_head(self) -> None:
        workflow = self.read('.github/workflows/ongrow-support-console-windows-x64.yml')
        self.assertIn('pull_request:', workflow)
        self.assertIn('github.event.pull_request.head.repo.full_name == github.repository', workflow)
        self.assertIn('SOURCE_SHA: "${{ github.event.pull_request.head.sha || github.sha }}"', workflow)
        self.assertEqual(workflow.count('ref: ${{ env.SOURCE_SHA }}'), 2)
        self.assertNotIn('$GITHUB_SHA', workflow)
        self.assertNotIn('$env:GITHUB_SHA', workflow)
        self.assertIn('source=$env:SOURCE_SHA', workflow)


if __name__ == "__main__":
    unittest.main(verbosity=2)
