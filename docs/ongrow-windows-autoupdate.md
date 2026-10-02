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

Console custom profile directories, including the shared `Programs` and `OnGROW`
ancestors, have uninstall-only `RemoveFolder` rows tied to its owned package
registry component. These compile to `RemoveFile` rows with NULL `FileName` and
mode 2, which remove only empty folders. They cannot delete any files or nonempty
shared directory; `LocalAppDataFolder` itself is never a cleanup target. There is
no wildcard file removal, recursive removal or ICE64 suppression.

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
Native run `36985921551` passed the 15 Python tests and compiled the inert Desk
schema, then stopped in table inspection because an optional-table conditional
unwrapped an empty result before its `Count` check. Optional table queries now
wrap the entire conditional in `@(...)`. The native adapter regression checks
absent tables and present tables with zero, one and three synthetic rows, exact
columns/content and view closure before any installer transaction. The portable
structural regression checks that these outer array expressions remain intact;
it does not execute PowerShell or prove native lifecycle success.
Run `37001581804` passed that adapter regression and the Desk schema checks, then
Console compilation rejected missing shared-ancestor cleanup with ICE64. The
revision adds empty-only declarations for those ancestors. Native inspection
checks NULL filenames, owned component, exact directory coverage and uninstall
mode. Two foreign sentinels in the shared ancestors must survive Console install,
upgrade, failed-update rollback, downgrade rejection and uninstall. Fixture cleanup
deletes only its exclusively created, unchanged sentinel files, never the shared
directories. Native lifecycle success remains unproven until the revised CI passes.
Mac Python PASS is not native Windows PASS. This layer also does not prove real
Desk service health, user-session exclusion, protected update staging, signed
manifest consumption or end-to-end automatic installation.

References: [WiX MajorUpgrade](https://docs.firegiant.com/wix/schema/wxs/majorupgrade/),
[ServiceControl](https://docs.firegiant.com/wix/schema/wxs/servicecontrol/),
[Windows Installer rollback](https://learn.microsoft.com/en-us/windows/win32/msi/rollback-installation),
[ICE64](https://learn.microsoft.com/en-us/windows/win32/msi/ice64),
[RemoveFile table](https://learn.microsoft.com/en-us/windows/win32/msi/removefile-table),
[PowerShell array subexpression](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_arrays#the-array-subexpression-operator).
