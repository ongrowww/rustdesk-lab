# OnGROW Support Desk branding

This directory contains the source and generator for the macOS app icon used
by the OnGROW Support Desk lab build.

## Brand source

- Source mark: `ongrow-mark.png`
- Source URL: `https://www.ongrow.de/logo.png`
- Retrieved: 2026-07-27
- SHA-256: `1be8f4e2320e1c226110b7c754a49240d7c6cc1eaad45bba5bdc9361da190389`
- Primary violet: `#7516F8`
- Secondary lime: `#DBF87C`

The source mark and colors belong to OnGROW GmbH. The generated icon places
the white OnGROW mark on a violet rounded square with a lime accent.

## Regenerate the macOS icon

Run on macOS:

```bash
swift branding/ongrow/generate_macos_icon.swift \
  branding/ongrow/ongrow-mark.png \
  flutter/macos/Runner/AppIcon.icns \
  branding/ongrow/ongrow-support-desk-icon.png
```

The script creates a temporary iconset, renders every required size and uses
Apple's `iconutil` to produce the final `.icns` file.
