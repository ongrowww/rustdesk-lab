#!/usr/bin/env python3
"""Local fixture checks; --integration performs real Sparkle replacement in CI only."""
import argparse
import functools
import hashlib
import http.server
import os
import plistlib
import shutil
import subprocess
import tempfile
import threading
import unittest
import uuid
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "scripts/fixtures/ongrow_sparkle_probe"
TAR_SHA256 = "c2bf58aa8387266ac179357b1415d6f2635f044da8be41042af32425dae6da0c"
SPM_SHA256 = "17e28312b8e18ab7cdbbe09a6fb28cc55a5479ec6c371dbc07cdecd2a14fd959"
SPARKLE_COMMIT = "eef1a539a373c1f1a320624b1130fc5de7b2e100"


def distribution_paths(distribution):
    framework = distribution / "Sparkle.framework"
    signer = distribution / "bin/sign_update"
    info = framework / "Versions/B/Resources/Info.plist"
    if not framework.is_dir() or framework.is_symlink() or not info.is_file():
        raise RuntimeError("pinned distribution framework layout missing")
    if not signer.is_file() or signer.is_symlink() or not os.access(signer, os.X_OK):
        raise RuntimeError("pinned distribution sign_update layout missing")
    if plistlib.loads(info.read_bytes()).get("CFBundleShortVersionString") != "2.10.0":
        raise RuntimeError("pinned distribution framework version mismatch")
    return framework, signer


def validate_cli_app(app):
    cli = app / "Contents/MacOS/sparkle"
    info = app / "Contents/Info.plist"
    if app.is_symlink() or not info.is_file() or not cli.is_file() or cli.is_symlink() or not os.access(cli, os.X_OK):
        raise RuntimeError("explicit built CLI application layout missing")
    if plistlib.loads(info.read_bytes()).get("CFBundleIdentifier") != "org.sparkle-project.sparkle-cli":
        raise RuntimeError("explicit built CLI application identity mismatch")
    framework = app / "Contents/Frameworks/Sparkle.framework"
    framework_info = framework / "Versions/B/Resources/Info.plist"
    if framework.is_symlink() or not framework_info.is_file() or plistlib.loads(
            framework_info.read_bytes()).get("CFBundleShortVersionString") != "2.10.0":
        raise RuntimeError("explicit built CLI framework version mismatch")
    return cli


def run(*args, capture=False):
    return subprocess.run([str(arg) for arg in args], check=True,
                          stdout=subprocess.PIPE if capture else None,
                          stderr=subprocess.PIPE if capture else None, text=True, timeout=120)


def sign_framework(framework):
    # Inside out, preserving the versioned framework symlinks. Never --deep sign.
    version = framework / "Versions/B"
    for relative in ("XPCServices/Downloader.xpc", "XPCServices/Installer.xpc",
                     "Autoupdate", "Updater.app"):
        target = version / relative
        if not target.exists():
            raise RuntimeError(f"pinned Sparkle helper missing: {relative}")
        run("codesign", "--force", "--sign", "-", "--timestamp=none", target)
    run("codesign", "--force", "--sign", "-", "--timestamp=none", framework)
    run("codesign", "--verify", "--deep", "--strict", framework)


def bundle(path, framework, executable, identity, key, version):
    contents = path / "Contents"
    (contents / "MacOS").mkdir(parents=True)
    (contents / "Frameworks").mkdir()
    shutil.copy2(executable, contents / "MacOS/probe")
    shutil.copytree(framework, contents / "Frameworks/Sparkle.framework", symlinks=True)
    info = {"CFBundleIdentifier": identity, "CFBundleName": "OnGROW updater probe",
            "CFBundleExecutable": "probe", "CFBundlePackageType": "APPL",
            "CFBundleVersion": str(version), "CFBundleShortVersionString": str(version),
            "LSMinimumSystemVersion": "12.3", "SUPublicEDKey": key,
            "SURequireSignedFeed": True, "SUVerifyUpdateBeforeExtraction": True,
            "SUSignedFeedFailureExpirationInterval": 0, "SUEnableSystemProfiling": False,
            "SUEnableAutomaticChecks": False,
            "NSAppTransportSecurity": {"NSAllowsLocalNetworking": True}}
    (contents / "Info.plist").write_bytes(plistlib.dumps(info))
    run("codesign", "--force", "--sign", "-", "--timestamp=none", path)
    run("codesign", "--verify", "--deep", "--strict", path)


class QuietHandler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *_args):
        pass


def integration(distribution, cli_app):
    if os.environ.get("GITHUB_ACTIONS") != "true" or os.environ.get("RUNNER_OS") != "macOS":
        raise RuntimeError("real replacement is allowed only on the disposable macOS CI runner")
    distribution = distribution.resolve(strict=True)
    framework, signer = distribution_paths(distribution)
    expected_cli_app = Path(os.environ["RUNNER_TEMP"]) / "sparkle-cli-build/Build/Products/Release/sparkle.app"
    if cli_app.resolve(strict=True) != expected_cli_app.resolve(strict=True):
        raise RuntimeError("CLI application is not the explicitly built runner output")
    validate_cli_app(cli_app)
    with tempfile.TemporaryDirectory(prefix="ongrow-sparkle-probe-", dir=os.environ["RUNNER_TEMP"]) as directory:
        task = Path(directory)
        task.chmod(0o700)
        run("xcrun", "swiftc", "-parse-as-library", FIXTURE / "keys.swift", "-o", task / "keys")
        run(task / "keys", task / "private.key", task / "public.key")
        run(task / "keys", task / "wrong.key", task / "wrong-public.key")
        key = (task / "public.key").read_text()
        sign_framework(framework)
        # The CLI target builds the same pinned source framework as a dependency.
        # Run the test against the checksummed release framework instead. Only
        # replace a copy owned by this fresh CI fixture directory.
        owned_cli_app = task / "sparkle.app"
        shutil.copytree(cli_app, owned_cli_app, symlinks=True)
        copied_framework = owned_cli_app / "Contents/Frameworks/Sparkle.framework"
        if copied_framework.is_symlink() or not copied_framework.is_dir():
            raise RuntimeError("owned copied CLI framework layout rejected")
        shutil.rmtree(copied_framework)
        shutil.copytree(framework, copied_framework, symlinks=True)
        release_binary = (framework / "Versions/B/Sparkle").read_bytes()
        if (copied_framework / "Versions/B/Sparkle").read_bytes() != release_binary:
            raise RuntimeError("CLI is not using the verified distribution framework")
        run("codesign", "--force", "--sign", "-", "--timestamp=none", owned_cli_app)
        run("codesign", "--verify", "--deep", "--strict", owned_cli_app)
        cli = validate_cli_app(owned_cli_app)
        for version in (0, 1, 2):
            run("xcrun", "swiftc", "-parse-as-library", "-target", "arm64-apple-macosx12.3",
                "-D", f"PROBE_V{version}", "-F", distribution, "-framework", "Sparkle", "-Xlinker", "-rpath",
                "-Xlinker", "@executable_path/../Frameworks", FIXTURE / "main.swift", "-o", task / f"probe-v{version}")
        # Compile the production controller against the same pinned framework.
        # This does not launch it or claim full Flutter app build proof.
        run("xcrun", "swiftc", "-typecheck", "-F", distribution,
            ROOT / "flutter/macos/Runner/OnGrowUpdatePolicy.swift",
            ROOT / "flutter/macos/Runner/OnGrowUpdater.swift")
        web = task / "web"; web.mkdir()
        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0),
            functools.partial(QuietHandler, directory=str(web)))
        thread = threading.Thread(target=server.serve_forever, daemon=True); thread.start()
        origin = f"http://127.0.0.1:{server.server_port}"
        try:
            for case in ("success", "wrong-key", "tampered-archive", "unsigned-feed", "tampered-feed", "older-version"):
                case_root = task / case; case_root.mkdir()
                identity = f"de.ongrow.updaterprobe.{uuid.uuid4().hex}"
                host = case_root / "Probe.app"
                next_app = web / "Probe.app"
                if next_app.exists(): shutil.rmtree(next_app)
                bundle(host, framework, task / "probe-v1", identity, key, 1)
                original_hash = hashlib.sha256((host / "Contents/MacOS/probe").read_bytes()).hexdigest()
                version = 0 if case == "older-version" else 2
                bundle(next_app, framework, task / f"probe-v{version}", identity, key, version)
                next_hash = hashlib.sha256((next_app / "Contents/MacOS/probe").read_bytes()).hexdigest()
                if next_hash == original_hash:
                    raise RuntimeError("probe versions must have distinct executable payloads")
                archive = web / "probe.zip"
                if archive.exists(): archive.unlink()
                run("ditto", "-c", "-k", "--sequesterRsrc", "--keepParent", next_app, archive)
                signing_key = task / ("wrong.key" if case == "wrong-key" else "private.key")
                signature = run(signer, "--ed-key-file", signing_key, "-p", archive, capture=True).stdout.strip()
                if case == "tampered-archive":
                    with archive.open("ab") as stream: stream.write(b"tamper")
                feed = web / "appcast.xml"
                feed.write_text('<?xml version="1.0" encoding="utf-8"?>\n'
                    '<rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle">'
                    '<channel><title>Isolated OnGROW probe</title><item><title>Probe update</title>'
                    f'<sparkle:version>{version}</sparkle:version>'
                    f'<enclosure url="{origin}/probe.zip" length="{archive.stat().st_size}" '
                    f'type="application/octet-stream" sparkle:edSignature="{signature}" />'
                    '</item></channel></rss>')
                if case != "unsigned-feed":
                    run(signer, "--ed-key-file", task / "private.key", "-p", feed, capture=True)
                if case == "tampered-feed":
                    feed.write_text(feed.read_text().replace("Probe update", "Changed update"))
                sentinel = case_root / "appdata-sentinel"
                sentinel.write_bytes(b"customer-config-must-survive")
                result = subprocess.run([str(cli), str(host), "--check-immediately",
                    "--feed-url", f"{origin}/appcast.xml", "--user-agent-name", "OnGROW isolated CI probe"],
                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, timeout=120)
                info = plistlib.loads((host / "Contents/Info.plist").read_bytes())
                expected = "2" if case == "success" else "1"
                if info["CFBundleVersion"] != expected or sentinel.read_bytes() != b"customer-config-must-survive":
                    raise RuntimeError(f"{case}: actual installed bundle or sentinel assertion failed")
                marker = run(host / "Contents/MacOS/probe", "--marker", capture=True).stdout.strip()
                installed_hash = hashlib.sha256((host / "Contents/MacOS/probe").read_bytes()).hexdigest()
                expected_hash = next_hash if case == "success" else original_hash
                if installed_hash != expected_hash:
                    raise RuntimeError(f"{case}: installed executable byte hash mismatch")
                if marker != expected:
                    raise RuntimeError(f"{case}: actual executable marker mismatch")
                if case == "success" and result.returncode != 0:
                    raise RuntimeError(f"real Sparkle installation failed, exit={result.returncode}: {result.stderr[-3000:]}")
                if case not in ("success", "older-version") and result.returncode == 0:
                    raise RuntimeError(f"{case}: rejection unexpectedly reported success")
                print(f"Sparkle {case}: actual bundle={expected}, marker={marker}, sentinel preserved, exit={result.returncode}")
        finally:
            server.shutdown(); server.server_close(); thread.join(timeout=10)


class FixtureTests(unittest.TestCase):
    def distribution_fixture(self, root):
        framework = root / "Sparkle.framework"
        info = framework / "Versions/B/Resources/Info.plist"
        info.parent.mkdir(parents=True)
        info.write_bytes(plistlib.dumps({"CFBundleShortVersionString": "2.10.0"}))
        signer = root / "bin/sign_update"
        signer.parent.mkdir(); signer.write_bytes(b"fixture"); signer.chmod(0o700)
        return framework, signer, info

    def cli_fixture(self, root):
        app = root / "sparkle.app"
        cli = app / "Contents/MacOS/sparkle"
        cli.parent.mkdir(parents=True); cli.write_bytes(b"fixture"); cli.chmod(0o700)
        info = app / "Contents/Info.plist"
        info.write_bytes(plistlib.dumps({"CFBundleIdentifier": "org.sparkle-project.sparkle-cli"}))
        framework_info = app / "Contents/Frameworks/Sparkle.framework/Versions/B/Resources/Info.plist"
        framework_info.parent.mkdir(parents=True)
        framework_info.write_bytes(plistlib.dumps({"CFBundleShortVersionString": "2.10.0"}))
        return app, cli, info, framework_info

    def test_release_has_no_cli_path_fallback(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            framework, signer, _ = self.distribution_fixture(root)
            self.assertEqual(distribution_paths(root), (framework, signer))
            self.assertFalse((root / "bin/sparkle.app").exists())

    def test_distribution_missing_or_wrong_layout_is_rejected(self):
        for case in ("missing-signer", "nonexecutable-signer", "wrong-version", "missing-framework", "symlink-signer"):
            with self.subTest(case=case), tempfile.TemporaryDirectory() as directory:
                root = Path(directory); framework, signer, info = self.distribution_fixture(root)
                if case == "missing-signer": signer.unlink()
                if case == "nonexecutable-signer": signer.chmod(0o600)
                if case == "wrong-version": info.write_bytes(plistlib.dumps({"CFBundleShortVersionString": "2.9.3"}))
                if case == "missing-framework": shutil.rmtree(framework)
                if case == "symlink-signer":
                    external = root / "external"; signer.rename(external); signer.symlink_to(external)
                with self.assertRaises(RuntimeError): distribution_paths(root)

    def test_cli_requires_exact_application_identity_and_framework(self):
        for case in ("valid", "missing-binary", "wrong-identity", "wrong-framework", "nonexecutable-binary"):
            with self.subTest(case=case), tempfile.TemporaryDirectory() as directory:
                app, cli, info, framework_info = self.cli_fixture(Path(directory))
                if case == "missing-binary": cli.unlink()
                if case == "nonexecutable-binary": cli.chmod(0o600)
                if case == "wrong-identity": info.write_bytes(plistlib.dumps({"CFBundleIdentifier": "foreign"}))
                if case == "wrong-framework": framework_info.write_bytes(plistlib.dumps({"CFBundleShortVersionString": "2.9.3"}))
                if case == "valid": self.assertEqual(validate_cli_app(app), cli)
                else:
                    with self.assertRaises(RuntimeError): validate_cli_app(app)

    def test_fixture_has_no_rustdesk_or_permission_code(self):
        code = (FIXTURE / "main.swift").read_text()
        self.assertIn('"--marker"', code)
        self.assertIn('#if PROBE_V2', code)
        self.assertIn('#elseif PROBE_V0', code)
        self.assertNotIn("Bundle.main", code)
        for forbidden in ("URLSession", "RustDesk", "AXIsProcessTrusted", "CGRequest", "LaunchAgent"):
            self.assertNotIn(forbidden, code)

    def test_private_key_generator_never_prints_secret(self):
        code = (FIXTURE / "keys.swift").read_text()
        self.assertNotIn("print(", code)
        self.assertIn("0o600", code)
        self.assertIn(".withoutOverwriting", code)

    def test_production_controller_cannot_start(self):
        code = (ROOT / "flutter/macos/Runner/OnGrowUpdater.swift").read_text()
        self.assertIn("let sessionBarrierReady = false", code)
        self.assertIn("startingUpdater: false", code)
        self.assertIn("guard OnGrowUpdatePolicy.mayStart", code)
        self.assertNotIn("UserDefaults", code)

    def test_pinned_workflow_is_isolated(self):
        code = (ROOT / ".github/workflows/ongrow-autoupdate-macos-lab.yml").read_text()
        self.assertIn(TAR_SHA256, code)
        self.assertIn(SPM_SHA256, code)
        self.assertIn(SPARKLE_COMMIT, code)
        self.assertIn('-scheme sparkle-cli', code)
        self.assertIn('test "$(git -C .sparkle-source rev-parse HEAD)" =', code)
        self.assertNotIn('make release', code)
        self.assertIn("github.event.pull_request.head.repo.full_name == github.repository", code)
        self.assertIn("contents: read", code)
        self.assertNotIn("secrets.", code)
        self.assertNotIn("sudo", code)
        self.assertNotIn("--deep --sign", code)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--integration", type=Path)
    parser.add_argument("--cli-app", type=Path)
    args, remaining = parser.parse_known_args()
    if args.integration:
        if args.cli_app is None:
            parser.error("--integration requires explicit --cli-app")
        integration(args.integration, args.cli_app)
    else:
        unittest.main(argv=[__file__, *remaining])
