#!/usr/bin/env python3
import base64
import json
import os
import plistlib
import tempfile
import unittest
from unittest import mock
from pathlib import Path
from apply_ongrow_macos_update_profile import PRODUCTS, apply

ROOT = Path(__file__).resolve().parents[1]
KEY = base64.b64encode(bytes(range(32))).decode()


class ProfileTests(unittest.TestCase):
    def fixture(self, root, role="customer-desk"):
        target = root / "flutter/macos/Runner"
        (target / "Configs").mkdir(parents=True)
        identity, name = PRODUCTS[role]
        info = {"CFBundleIdentifier": "$(PRODUCT_BUNDLE_IDENTIFIER)",
                "CFBundleDisplayName": name, "CFBundleVersion": "$(FLUTTER_BUILD_NUMBER)",
                "OnGrowUpdateApplyEnabled": False}
        (target / "Info.plist").write_bytes(plistlib.dumps(info))
        (target / "Configs/AppInfo.xcconfig").write_text(
            f"PRODUCT_NAME = {name}\nPRODUCT_BUNDLE_IDENTIFIER = {identity}\n")
        return target / "Info.plist"

    def test_both_products_remain_install_locked(self):
        for role in PRODUCTS:
            with self.subTest(role=role), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                path = self.fixture(root, role)
                feed = f"https://ongrow.de/assets/updates/{role}/macos-arm64/appcast.xml"
                apply(root, role, feed, KEY, "12")
                info = plistlib.loads(path.read_bytes())
                self.assertEqual(info["SUFeedURL"], feed)
                self.assertEqual(info["CFBundleVersion"], "1.0.12")
                self.assertEqual(info["OnGrowUpdateSequence"], "12")
                self.assertFalse(info["OnGrowUpdateApplyEnabled"])
                self.assertTrue(info["SURequireSignedFeed"])
                self.assertTrue(info["SUVerifyUpdateBeforeExtraction"])
                self.assertEqual(info["SUSignedFeedFailureExpirationInterval"], 0)
                self.assertFalse(info["SUEnableSystemProfiling"])

    def test_bad_input_is_unchanged(self):
        good = ["customer-desk", "https://ongrow.de/assets/updates/customer-desk/macos-arm64/appcast.xml", KEY, "1"]
        cases = [(0, "support-console"), (0, "unknown"), (1, "http://ongrow.de/feed"),
                 (1, good[1] + "?x=1"), (1, good[1] + "#fragment"),
                 (1, good[1].replace("ongrow.de", "user@ongrow.de")),
                 (1, good[1].replace("customer-desk", "support-console")),
                 (1, good[1].replace("assets", "%61ssets")),
                 (2, "broken"), (2, base64.b64encode(bytes(31)).decode()),
                 (3, "0"), (3, "-1"), (3, "01"), (3, "1000000000"), (3, "99990000")]
        for index, value in cases:
            with self.subTest(value=value), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                path = self.fixture(root)
                before = path.read_bytes()
                args = good.copy(); args[index] = value
                with self.assertRaises(ValueError):
                    apply(root, *args)
                self.assertEqual(path.read_bytes(), before)

    def test_missing_duplicate_and_preconfigured_anchors(self):
        for kind in ("missing", "duplicate", "already", "config-duplicate", "apply-enabled"):
            with self.subTest(kind=kind), tempfile.TemporaryDirectory() as directory:
                root = Path(directory); path = self.fixture(root)
                info = plistlib.loads(path.read_bytes())
                if kind == "missing": del info["CFBundleVersion"]
                if kind == "already": info["SUFeedURL"] = "foreign"
                if kind == "apply-enabled": info["OnGrowUpdateApplyEnabled"] = True
                path.write_bytes(plistlib.dumps(info))
                if kind == "duplicate":
                    path.write_bytes(path.read_bytes().replace(b"</dict>", b"<key>CFBundleVersion</key><string>1</string></dict>"))
                if kind == "config-duplicate":
                    config = path.parent / "Configs/AppInfo.xcconfig"
                    config.write_text(config.read_text() + "PRODUCT_NAME = OnGROW Support Desk\n")
                before = path.read_bytes()
                with self.assertRaises(ValueError):
                    apply(root, "customer-desk", "https://ongrow.de/assets/updates/customer-desk/macos-arm64/appcast.xml", KEY, "1")
                self.assertEqual(path.read_bytes(), before)

    def test_pinned_dependency_and_embed(self):
        project = (ROOT / "flutter/macos/Runner.xcodeproj/project.pbxproj").read_text()
        pin = json.loads((ROOT / "flutter/macos/Runner.xcodeproj/project.xcworkspace/xcshareddata/swiftpm/Package.resolved").read_text())
        revision = "eef1a539a373c1f1a320624b1130fc5de7b2e100"
        self.assertEqual(pin["pins"][0]["state"]["revision"], revision)
        self.assertIn(f"kind = revision; revision = {revision}", project)
        self.assertIn("Sparkle in Embed Libraries", project)
        self.assertIn("CodeSignOnCopy, RemoveHeadersOnCopy", project)
        for source in ("OnGrowUpdater.swift", "OnGrowUpdatePolicy.swift"):
            self.assertIn(f"{source} in Sources", project)

    def test_apple_tools_run_only_on_apple_runners_before_build(self):
        for role in ("lab", "support-console"):
            workflow = (ROOT / f".github/workflows/ongrow-{role}-macos-arm64.yml").read_text()
            bridge, native = workflow.split("  build-macos-arm64:", 1)
            self.assertIn("runs-on: ubuntu-22.04", bridge)
            self.assertNotIn("xcrun", bridge)
            self.assertNotIn("plutil", bridge)
            self.assertIn("runs-on: macos-14", native)
            verification = native.index("- name: Verify locked macOS update integration")
            self.assertLess(verification, native.index("- name: Build RustDesk"))
            self.assertIn("xcrun swiftc", native[verification:])
            self.assertIn("plutil -lint", native[verification:])

    def test_release_counter_boundaries(self):
        previous = None
        for counter in (1, 99, 100, 9999, 10000, 99_989_999):
            with tempfile.TemporaryDirectory() as directory:
                root = Path(directory); path = self.fixture(root)
                apply(root, "customer-desk", "https://ongrow.de/assets/updates/customer-desk/macos-arm64/appcast.xml", KEY, str(counter))
                version = plistlib.loads(path.read_bytes())["CFBundleVersion"]
                components = tuple(map(int, version.split(".")))
                self.assertLessEqual(components[0], 9999)
                self.assertLessEqual(components[1], 99)
                self.assertLessEqual(components[2], 99)
                if previous is not None: self.assertGreater(components, previous)
                previous = components

    def test_shared_or_symlink_inputs_are_rejected(self):
        for kind in ("symlink", "hardlink", "config-symlink"):
            with self.subTest(kind=kind), tempfile.TemporaryDirectory() as directory:
                root = Path(directory); path = self.fixture(root)
                original = path.read_bytes()
                if kind == "config-symlink":
                    config = path.parent / "Configs/AppInfo.xcconfig"
                    external = root / "external-config"; config.rename(external)
                    config.symlink_to(external)
                else:
                    external = root / "external-plist"
                    if kind == "hardlink": os.link(path, external)
                    else:
                        path.rename(external); path.symlink_to(external)
                with self.assertRaises(ValueError):
                    apply(root, "customer-desk", "https://ongrow.de/assets/updates/customer-desk/macos-arm64/appcast.xml", KEY, "1")
                self.assertEqual(path.read_bytes(), original)

    def test_io_failure_does_not_truncate_original(self):
        for operation in ("os.fsync", "os.replace"):
            with self.subTest(operation=operation), tempfile.TemporaryDirectory() as directory:
                root = Path(directory); path = self.fixture(root)
                original = path.read_bytes()
                with mock.patch(f"apply_ongrow_macos_update_profile.{operation}", side_effect=OSError("injected failure")):
                    with self.assertRaises(OSError):
                        apply(root, "customer-desk", "https://ongrow.de/assets/updates/customer-desk/macos-arm64/appcast.xml", KEY, "1")
                self.assertEqual(path.read_bytes(), original)
                self.assertEqual(list(path.parent.glob(".ongrow-update-profile-*")), [])


if __name__ == "__main__":
    unittest.main()
