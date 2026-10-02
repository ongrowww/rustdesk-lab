# Windows transactional update packages

This package layer does not enable automatic downloads or installation. The trusted
runtime scheduler, signed staging, session coordination, health recovery and the
one-time legacy EXE migration belong to the following integration step.

The generator uses an independent WiX 4.0.6 project, not the upstream RustDesk
custom actions. It stages a complete validated AMD64 payload exclusively, checks
PE roles, product identity, provenance, links and Windows filename aliases, and
writes its completion descriptor last. There is no signing key input, installer
execution, device registration or server contact. `--emit-only` works without WiX.

```sh
python3 scripts/package_ongrow_windows_msi.py --source VERIFIED_DESK_TREE \
  --output NEW_OUTPUT_DIRECTORY --product customer-desk --sequence 42 \
  --upstream-version 1.4.9 --source-sha EXACT_40_CHARACTER_SOURCE_SHA --emit-only
```

Without `--emit-only`, the explicit output compiles with `dotnet build`. Windows
CI uses a pinned runner-only .NET SDK and WiX SDK; no Mac system installation is
needed. An incomplete output is intentionally retained and never publishable.

## Fixed ownership

- Customer Desk: per-machine, `ProgramFiles64Folder/OnGROW Support Desk`, one
  declarative LocalSystem service `OnGROW Support Desk`, argument `--service`.
- Support Console: per-user, `LocalAppDataFolder/Programs/OnGROW/Support Console`,
  HKCU URI `ongrow-support-console`, no service, system policy or UAC custom action.

Packages own only their payload and package metadata. Existing configurations,
device identities, grants, vault data, drivers, firewall and RustDesk installations
are not read, migrated or removed. A non-MSI executable or legacy uninstall
registration at the product target blocks silent installation before mutation.
Legacy migration requires a separate trusted bootstrap; do not bypass that guard.

The bounded positive release counter is encoded losslessly as
`floor(sequence/16777216).floor(sequence/65536)%256.sequence%65536`.
Range: 1 through 4,294,967,295; MSI bounds: 255.255.65535. Thus release order does
not depend on upstream `1.4.9`. UpgradeCode and file-component GUIDs are stable per
profile/path; ProductCode changes per release/source. Probe identities are in a
different deterministic namespace, never device identifiers.

Major upgrades remove the previous product after InstallInitialize, inside the
rollback transaction. Downgrades and same-version upgrades are not enabled.
Files and registry are declarative; service installation is vital, control waits.
Rollback-disabled systems are rejected. There are no production custom actions,
ignored service errors, XCOPY, batch copy, foreign broker or shared-driver actions.
MSI rollback is not a substitute for post-commit application-health recovery.

## Evidence and limitations

`python3 scripts/test_ongrow_windows_msi.py` validates actual emitted XML and
payload rejection/staging. It is not proof of native install or rollback.

The dedicated `ongrow-autoupdate-windows-lab.yml` runs on a disposable elevated
Windows runner. It compiles inert production-schema fixtures only for native MSI
table inspection; it **never installs production-profile MSI packages**. Actual
install/upgrade/failure/rollback/downgrade/uninstall uses a C# fixture with no
networking or application configuration, separate `OnGROW MSI Probe` names,
upgrade identities, directories, registry and URI. Only this probe has a checked
deferred failure action. The drill compares executable versions, registry, data,
running service and an external app-data sentinel. It has native exit gates and
bounded waits and only uninstalls known probe ProductCodes.
The service writes its actually loaded assembly version on `OnStart` into only
the probe-owned registry value; assertions check that marker after both upgrade
and rollback instead of inferring the running version from a file on disk.

Until the native CI run passes, Windows lifecycle evidence remains pending.
Mac Python PASS is not native Windows PASS. This layer also does not prove real
Desk service health, user-session exclusion, protected update staging, signed
manifest consumption or end-to-end automatic installation.

References: [WiX MajorUpgrade](https://docs.firegiant.com/wix/schema/wxs/majorupgrade/),
[ServiceControl](https://docs.firegiant.com/wix/schema/wxs/servicecontrol/),
[Windows Installer rollback](https://learn.microsoft.com/en-us/windows/win32/msi/rollback-installation).
