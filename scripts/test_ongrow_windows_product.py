#!/usr/bin/env python3
"""Static product-boundary checks for the Windows customer desk."""

from __future__ import annotations

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


class WindowsProductTests(unittest.TestCase):
    def read(self, relative: str) -> str:
        return (ROOT / relative).read_text(encoding="utf-8")

    def test_runner_has_ongrow_binary_and_version_identity(self) -> None:
        cmake = self.read("flutter/windows/CMakeLists.txt")
        runner_cmake = self.read("flutter/windows/runner/CMakeLists.txt")
        resources = self.read("flutter/windows/runner/Runner.rc")
        main = self.read("flutter/windows/runner/main.cpp")

        self.assertIn('project(ongrow_support_desk LANGUAGES CXX)', cmake)
        self.assertIn('set(BINARY_NAME "ongrow_support_desk")', cmake)
        self.assertIn(
            'set_target_properties(${BINARY_NAME} PROPERTIES OUTPUT_NAME "OnGROW Support Desk")',
            runner_cmake,
        )
        self.assertIn('VALUE "CompanyName", "OnGROW GmbH"', resources)
        self.assertIn('VALUE "ProductName", "OnGROW Support Desk"', resources)
        self.assertIn('VALUE "InternalName", "ongrow_support_desk"', resources)
        self.assertIn(
            'VALUE "OriginalFilename", "OnGROW Support Desk.exe"', resources
        )
        self.assertNotIn('VALUE "CompanyName", "Purslane', resources)
        self.assertNotIn('VALUE "ProductName", "RustDesk"', resources)
        self.assertIn('std::wstring app_name = L"OnGROW Support Desk";', main)

    def test_customer_role_routes_to_restricted_home_and_uri_scheme(self) -> None:
        home = self.read("flutter/lib/desktop/pages/desktop_home_page.dart")
        common = self.read("src/common.rs")

        role_guard = "bind.mainGetAppNameSync() == 'OnGROW Support Desk'"
        self.assertIn(role_guard, home)
        self.assertIn("return const OnGrowSupportHome();", home)
        self.assertLess(home.index(role_guard), home.index("buildRightPane(context)"))
        self.assertIn('"ongrow-support://".to_owned()', common)
        self.assertIn(
            "let url_scheme = crate::get_uri_prefix()",
            self.read("src/platform/windows.rs"),
        )

    def test_pe_filename_matches_installer_and_updater_contract(self) -> None:
        runner_cmake = self.read("flutter/windows/runner/CMakeLists.txt")
        windows = self.read("src/platform/windows.rs")
        updater = self.read("src/updater.rs")

        output = re.search(r'OUTPUT_NAME "([^"]+)"', runner_cmake)
        self.assertIsNotNone(output)
        actual_filename = f"{output.group(1)}.exe"
        expected_filename = "OnGROW Support Desk.exe"
        self.assertEqual(actual_filename.casefold(), expected_filename.casefold())
        self.assertIn(
            'format!("{}\\\\{}.exe", path, crate::get_app_name())', windows
        )
        self.assertIn(
            'format!("{}.exe", crate::get_app_name().to_lowercase())', updater
        )

    def test_custom_product_does_not_consider_legacy_uninstall_key(self) -> None:
        windows = self.read("src/platform/windows.rs")

        self.assertIn('if app_name == "RustDesk" {', windows)
        self.assertIn(
            "custom_product_uninstall_candidates_never_use_rustdesk_legacy_key",
            windows,
        )
        self.assertIn("custom_product_service_commands_quote_names", windows)
        self.assertIn("sc create {app_name_arg}", windows)
        self.assertNotRegex(
            windows,
            r"(?:sc (?:create|start|stop|delete)|taskkill /F /IM) \{app_name\}",
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)
