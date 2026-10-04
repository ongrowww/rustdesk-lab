# Native installer admission integration

This is a disposable CI integration test, not a production updater.
The reviewed MSI and Sparkle installers now use a guardian outside the probe
bundle/service being replaced. The guardian imports the original Rust gate.
Its test-only factory acquires the original exclusive OS lock, calls
mark_pending synchronously and constructs the real Transaction from that lock.
No copy of the lock protocol, PID check or file-existence heuristic substitutes
for admission. Production APIs and VerifiedHealth construction remain unchanged.

## Contract

Only a fresh synthetic test root may bootstrap Ready. An actual session child
blocks Begin before any mutating installer command starts. Once the session
ends, Pending is durable before native process launch. The guardian stays alive
before start, while the actual installer caller is running, after caller return
and through the installation assertions. A second native process tests the real
gate while the caller is alive. Explicit end or guardian crash releases only
the kernel lock; admission remains Pending.

The Windows drill covers each separately identified Console/Desk probe product,
real v1/v2 MSI exchange, registry/files, Desk loaded service version and original
sentinels. It crashes the actual Transaction owner after the held v2 assertions,
then tests real failed-v3 rollback under the original exclusive recovery lease.
The recovery lease grants no package, installer, Ready or cleanup authority.
The CI-only adapter identifies disposable probe commands/products separately.
msiexec is awaited with its existing deadline and never killed by the guardian.

The macOS drill retains the exact Sparkle 2.10.0 pin, both checksums and official
CLI scheme, signed feed/archive validation and all six existing native cases.
Guarded success verifies actual v2 replacement, distinct executable bytes,
an explicitly invoked marker and independent sentinel while ownership remains
held. A separate owner crash happens after Pending but before SDK start.
That case proves no installation began and v1 remains unchanged. It does not
prove behavior of a still-running Sparkle installer after caller/guardian crash.
Manual marker invocation is not an SDK relaunch.

The Mac transport accepts unmodified native SDK redirects, including redirects
outside the initial product prefix. Fixed initial feed, future initial archive
validation, mandatory Ed25519 signatures against the baked-in key, disabled
signing fallback and normal HTTPS/ATS remain required. No credentials or personal
data accompany these requests. There is no strict all-redirect allowlist claim
and no new Rust Mac-manifest pipeline.

## Probe isolation

The new Rust tests are compiled only under test plus ongrow_native_apply_probe.
An ordinary app import contains none of these fixture tests or runtime inputs.
The CI adapter accepts no arbitrary installer argv or root. Windows uses the
existing OS KnownFolder validator before any state write. Both CI workflows
require trusted exact-SHA checkout, read-only contents and the pinned Rust
1.81.0 toolchain before running the same local gate tests used in review.

Protocol input/output sizes, handshakes, pipe waits and own-child lifetimes are
bounded. Own test children are killed and waited on during failed cleanup.
A native MSI transaction is not killed. Sparkle caller timeout does not imply
that SDK helpers have stopped or that installation is quiescent. Neither SDK
cycle completion nor a new file hash can clear Pending. Fixture disposal on a
throwaway runner is not production recovery.

Unknown post-start caller, pipe, decoder or deadline failures retain the entire
owned Mac fixture and every gate/build/state root until the disposable runner
ends. Retention disables TemporaryDirectory finalizers as well as explicit
cleanup. This applies to the original six native SDK cases too. Windows marks
native start before msiexec launch and retains its Pending probe roots for the
whole runner lifetime, even on success, so later downgrade/uninstall failures
cannot lose them. A timed-out msiexec triggers no cleanup MSI.
Retention flags only control fixture disposal; they grant no admission, package,
Ready, health or installer-quiescence authority.

Local regressions exercise actual Transaction owners, context exit and garbage
collection, then verify retained roots and actual Pending admission. Synthetic
caller timeout/UTF-8 decoder/pipe failures cover the whole Mac fixture without
launching any native installer. These are retention tests, not installation proof.

Local verification runs at least eight substantive native tests twice, plus
the real Python pipe adapter, missing input and ordinary app import. The
existing Pending/store/runtime/session/profile/isolation/signature gates stay
in force. The recovery workflow freezes the whole repository against reviewed
8be346aff0a8a423af51c85d0faa6d66f05c5b07 with concrete scope paths only;
Git fixtures prove that Verifier, original OS gate and Cargo edits still fail.

## Remaining activation gates

Native CI must pass for both workflows on the exact reviewed pushed SHA, with
NATIVE_WINDOWS_GUARDED_APPLY_PASS and NATIVE_MACOS_GUARDED_APPLY_PASS after the
actual assertions. Local/source tests cannot replace those runs.

Still missing are a persistent authenticated update attempt with recheckable
package authority, production guardian/app/service integration, authentic new
process health and application/service quiescence, protected sequence commit
before Ready, discard/recovery, Desk SYSTEM/cross-principal probes and an actual
automatic scheduler/bootstrap. Sparkle's public signed appcast-item state and
protected attempt record can avoid duplicating the native verification path.
Health cannot be inferred from SDK child-process exit alone.

OnGrowUpdater's compiled sessionBarrierReady=false, startingUpdater=false and
OnGrowUpdateApplyEnabled=false remain unchanged. Developer ID signing and
notarization are separate customer-release prerequisites. This test ends every
installation Pending.
