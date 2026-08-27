#!/usr/bin/env python3
"""Static product-boundary checks for the Windows customer desk."""

from __future__ import annotations

import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


class WindowsProductTests(unittest.TestCase):
    def read(self, relative: str) -> str:
        return (ROOT / relative).read_text(encoding="utf-8")

    def test_runner_has_ongrow_binary_and_version_identity(self) -> None:
        cmake = self.read("flutter/windows/CMakeLists.txt")
        resources = self.read("flutter/windows/runner/Runner.rc")
        main = self.read("flutter/windows/runner/main.cpp")

        self.assertIn('project(ongrow_support_desk LANGUAGES CXX)', cmake)
        self.assertIn('set(BINARY_NAME "ongrow_support_desk")', cmake)
        self.assertIn('VALUE "CompanyName", "OnGROW GmbH"', resources)
        self.assertIn('VALUE "ProductName", "OnGROW Support Desk"', resources)
        self.assertIn('VALUE "InternalName", "ongrow_support_desk"', resources)
        self.assertIn(
            'VALUE "OriginalFilename", "ongrow_support_desk.exe"', resources
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

    def test_custom_product_does_not_consider_legacy_uninstall_key(self) -> None:
        windows = self.read("src/platform/windows.rs")

        self.assertIn('if app_name == "RustDesk" {', windows)
        self.assertIn(
            "custom_product_uninstall_candidates_never_use_rustdesk_legacy_key",
            windows,
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)
