#!/usr/bin/env python3
"""Convert the checked-in Support Desk macOS metadata into Support Console metadata."""

from pathlib import Path


REPLACEMENTS = {
    Path("build.py"): [
        ("OnGROW Support Desk.app/Contents/MacOS/", "OnGROW Support Console.app/Contents/MacOS/"),
    ],
    Path("flutter/macos/Runner/Configs/AppInfo.xcconfig"): [
        ("PRODUCT_NAME = OnGROW Support Desk", "PRODUCT_NAME = OnGROW Support Console"),
        ("PRODUCT_BUNDLE_IDENTIFIER = de.ongrow.supportdesk", "PRODUCT_BUNDLE_IDENTIFIER = de.ongrow.supportconsole"),
    ],
    Path("flutter/macos/Runner/Info.plist"): [
        ("<string>OnGROW Support Desk</string>", "<string>OnGROW Support Console</string>"),
        ("<string>de.ongrow.supportdesk</string>", "<string>de.ongrow.supportconsole</string>"),
        ("<string>ongrow-support</string>", "<string>ongrow-support-console</string>"),
        ("<key>LSUIElement</key>\n    <string>1</string>", "<key>LSUIElement</key>\n    <string>0</string>"),
    ],
    Path("flutter/macos/Runner.xcodeproj/project.pbxproj"): [
        ("OnGROW Support Desk.app", "OnGROW Support Console.app"),
        ("de.ongrow.supportdesk", "de.ongrow.supportconsole"),
    ],
    Path("flutter/macos/Runner.xcodeproj/xcshareddata/xcschemes/Runner.xcscheme"): [
        ("OnGROW Support Desk.app", "OnGROW Support Console.app"),
    ],
}


def main() -> int:
    for path, replacements in REPLACEMENTS.items():
        source = path.read_text(encoding="utf-8")
        for old, new in replacements:
            count = source.count(old)
            if count < 1:
                raise SystemExit(f"macOS product metadata rejected: missing {old!r} in {path}")
            source = source.replace(old, new)
        path.write_text(source, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
