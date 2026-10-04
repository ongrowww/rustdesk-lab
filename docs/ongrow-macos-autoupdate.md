# macOS updates with Sparkle

Both macOS products link Sparkle 2.10.0. Swift Package Manager is pinned to
commit `eef1a539a373c1f1a320624b1130fc5de7b2e100`. The binary checksum is
`17e28312b8e18ab7cdbbe09a6fb28cc55a5479ec6c371dbc07cdecd2a14fd959`.
Change the commit, package resolution and CI release checksums together.

## Activation boundary

The production controller does not start. `sessionBarrierReady` is a compiled
`false` in `OnGrowUpdater.swift`; `OnGrowUpdateApplyEnabled` is `false` in the
plist. Environment variables and user preferences cannot enable installation.
Unconfigured applications make no Sparkle update requests.

Sparkle's relaunch postponement delegate does not cover every installation
path. A connection snapshot also cannot prevent new sessions during replacement.
Before activation, implement a process-wide session admission barrier, durable
update ownership, service stop/restart and post-install health checks. Test
incoming and outgoing sessions racing against installation, failure recovery
and appdata preservation. Do not remove the gate until those tests pass. Keep
the independent Rust upstream updater blocked.
The native Mac transport deliberately accepts the unmodified Sparkle redirect
policy. The initial feed is fixed to the OnGROW product path; before production
activation, validate the initial archive enclosure URL against that product's
origin/prefix too. Redirects followed by the native SDK may leave that prefix.
This is not a strict runtime redirect allowlist. Both feed and archive require
Ed25519 signatures checked against the baked-in public key, with signing-failure
fallback disabled and normal HTTPS/ATS checks retained. Requests carry no
credentials or personal data. The build profile's strict initial URL validation
does not constrain every network redirect. Windows transport is unchanged.

## Public build profile

`scripts/apply_ongrow_macos_update_profile.py` validates all inputs and metadata
before atomically replacing the plist with an owned same-directory temporary
file. Run the existing Console product conversion first
when preparing Console metadata. The separate allowlisted feed paths are:

- Desk: `https://ongrow.de/assets/updates/customer-desk/macos-arm64/appcast.xml`
- Console: `https://ongrow.de/assets/updates/support-console/macos-arm64/appcast.xml`

These are accepted paths, not published endpoints or default configuration.
No real public key is committed. Future release builds must supply a 32-byte
base64 public key and monotonic release sequence from 1 to 99,989,999. Sequence
12 becomes `CFBundleVersion` 1.0.12. Components use a four/two/two-digit format;
the upstream display version can stay unchanged. `OnGrowUpdateSequence`
records the separate release counter.

Both feed and archive signatures are required. Validation happens before
extraction. A zero `SUSignedFeedFailureExpirationInterval` disables the signing
failure fallback. System profiling is disabled. No external release notes or
custom JavaScript are configured. Automatic checks/downloads are plist
defaults, not user preferences overwritten on each launch. The closed start
gate currently blocks the whole cycle, including manual installation.

Sparkle archive signatures differ from the domain-prefixed Windows manifest
signatures. Real signing and feed publication require a separate approved
release operation. This package does not use existing keys or the Keychain.

## Isolated CI drill

The dedicated macOS 14 workflow uses checksummed Sparkle tools, distinct
`de.ongrow.updaterprobe.*` bundle identities, temporary install roots and
ephemeral file-based 0600 keys. Its HTTP server listens only on loopback. HTTP
is allowed only in the fixture, never in a production profile.

Sparkle 2.10.0's release archive does not contain `sparkle-cli`. CI separately
checks out exact commit `eef1a539a373c1f1a320624b1130fc5de7b2e100`, verifies
HEAD and builds only the official `sparkle-cli` scheme. Its output must be
`$RUNNER_TEMP/sparkle-cli-build/Build/Products/Release/sparkle.app`.
There is no search or fallback executable. The CLI's copied embedded framework
is replaced, only within the newly owned fixture directory, by the verified
2.10.0 distribution framework. Version and framework binary bytes must match
before the copied CLI is ad-hoc signed and verified. The probe and controller
typecheck use that same release framework. No `make release`, packaging script,
global installation or local source build runs.

The actual Sparkle CLI downloads and replaces probe v1 with signed v2. Tests
inspect the installed bundle, run a distinct compiled executable marker,
compare the installed executable's byte hash and verify an
independent appdata sentinel. Wrong archive key, altered archive, unsigned or
altered feed and an older version must leave v1 intact. No RustDesk process,
service, enrollment, permission request, customer app or public server runs.

Local Python fixture tests do not prove installation. The CI drill typechecks
the production controller against the pinned framework, but it does not prove
a full Flutter build or GUI-controlled update. Both regular Mac workflows
keep their existing full build/launch checks. Nested helpers are signed inside
out, then the framework and app. Preserve framework symlinks. `--deep` is used
for verification, never for signing.

## Guardian integration boundary

The native CI drill additionally puts a real Rust Transaction outside the
replaced probe bundle. An actual session child blocks the whole mutating SDK
start and leaves v1 bytes unchanged. After that session ends, the guardian
durably records Pending before starting the actual Sparkle CLI. Another process
is denied admission while the CLI runs; ownership is retained through the v2
bundle/hash/explicit-marker/sentinel assertions, not merely until download or
cycle completion. Controlled guardian exit still leaves admission Pending.

A separate case crashes the Transaction owner after Pending and before any SDK
installer starts. It verifies unchanged v1 and Pending admission. It does not
claim an installer-after-caller-crash or quiescence proof. The explicit marker
execution is manual test instrumentation, never an SDK relaunch or VerifiedHealth.
SDK callbacks, caller exit and new bytes cannot authorize Ready. The existing
six replacement/signature/version cases remain.

Local checks:

```sh
python3 scripts/test_ongrow_macos_update_profile.py
python3 scripts/test_ongrow_sparkle_probe.py
xcrun swiftc flutter/macos/Runner/OnGrowUpdatePolicy.swift scripts/test_ongrow_macos_update_policy.swift -o /an/isolated/temp/path/policy-test
/an/isolated/temp/path/policy-test
```

## Customer release prerequisites

Current Lab builds are ad-hoc signed. OnGROW needs an Apple Developer Program
account, Developer ID signing and notarization for customer release. Ad-hoc
signing does not guarantee stable privacy permission recognition across
updates. This package does not weaken Gatekeeper, SIP, TCC, firewall or library
validation and adds no entitlement exception.

Copy the initial app into Applications. Read-only DMG and App Translocation
paths cannot be updated normally. A privileged installation may still require
macOS authorization; do not promise silent installation across that boundary.

References: [setup](https://sparkle-project.org/documentation/),
[programmatic setup](https://sparkle-project.org/documentation/programmatic-setup/),
[configuration](https://sparkle-project.org/documentation/customization/),
[publishing](https://sparkle-project.org/documentation/publishing/),
[Sparkle CLI](https://sparkle-project.org/documentation/sparkle-cli/).
