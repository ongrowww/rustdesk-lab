# Native, disposable GitHub Windows runner only. Never run against customer targets.
[CmdletBinding()]
param([Parameter(Mandatory)][string]$WorkDirectory)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_OS -ne 'Windows') { throw 'Disposable Windows CI required' }
if (-not [Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Isolated MSI service probe requires an elevated runner' }
$root = [IO.Path]::GetFullPath($WorkDirectory)
if (Test-Path $root) { throw 'Probe output already exists' }
$null = New-Item -ItemType Directory -Path $root
$csc = Join-Path $env:WINDIR 'Microsoft.NET/Framework64/v4.0.30319/csc.exe'
if (-not (Test-Path $csc -PathType Leaf)) { throw 'Pinned framework fixture compiler missing' }
$sha = (& git rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0 -or $sha -notmatch '^[0-9a-f]{40}$') { throw 'Invalid source SHA' }
$fixture = [IO.File]::ReadAllText((Join-Path $PSScriptRoot 'fixtures/ongrow_msi_probe.cs'))
$installer = New-Object -ComObject WindowsInstaller.Installer

function Read-Rows($Database, [string]$Sql, [string[]]$Columns) {
    $view = $Database.OpenView($Sql)
    try {
        $view.Execute()
        while ($null -ne ($record = $view.Fetch())) {
            $values = [ordered]@{}
            for ($i = 0; $i -lt $Columns.Count; $i++) { $values[$Columns[$i]] = $record.StringData($i + 1) }
            [PSCustomObject]$values
        }
    } finally { $view.Close() }
}

function Assert-Tables([string]$Msi, [string]$Product, [bool]$Probe, [int]$Sequence) {
    $db = $installer.OpenDatabase($Msi, 0)
    $props = @{}
    foreach ($row in (Read-Rows $db 'SELECT `Property`, `Value` FROM `Property`' @('Name','Value'))) { $props[$row.Name] = $row.Value }
    $name = if ($Product -eq 'customer-desk') { if ($Probe) { 'OnGROW MSI Probe Desk' } else { 'OnGROW Support Desk' } } else { if ($Probe) { 'OnGROW MSI Probe Console' } else { 'OnGROW Support Console' } }
    if ($props.ProductName -ne $name -or $props.ProductVersion -ne "0.0.$Sequence") { throw 'MSI product/version mismatch' }
    if ($Product -eq 'customer-desk') {
        if ($props.ALLUSERS -ne '1') { throw 'Desk is not per-machine' }
    } elseif ($props.ContainsKey('ALLUSERS') -and $props.ALLUSERS -eq '1') { throw 'Console became per-machine' }
    $tableNames = @(Read-Rows $db 'SELECT `Name` FROM `_Tables`' @('Name') | ForEach-Object Name)
    $actions = if ($tableNames -contains 'CustomAction') { @(Read-Rows $db 'SELECT `Action`, `Type`, `Source`, `Target` FROM `CustomAction`' @('Action','Type','Source','Target')) } else { @() }
    if (-not $Probe -and $actions.Count -ne 0) { throw 'Production MSI has a custom action' }
    if ($Probe -and ($actions.Count -ne 1 -or $actions[0].Action -ne 'ProbeFailAfterWrite' -or $actions[0].Target -ne '--fail-update' -or ([int]$actions[0].Type -band 1024) -eq 0 -or ([int]$actions[0].Type -band 64) -ne 0)) { throw 'Probe action does not fail transactionally' }
    $exec = @(Read-Rows $db 'SELECT `Action`, `Sequence` FROM `InstallExecuteSequence`' @('Action','Sequence'))
    $start = [int]($exec | Where-Object Action -eq 'InstallInitialize').Sequence
    $remove = [int]($exec | Where-Object Action -eq 'RemoveExistingProducts').Sequence
    $process = [int]($exec | Where-Object Action -eq 'ProcessComponents').Sequence
    if ($remove -le $start -or $remove -ge $process) { throw 'Major upgrade is outside rollback transaction' }
    if (-not ($exec | Where-Object Action -eq 'LaunchConditions')) { throw 'Silent execution missing legacy launch guard' }
    $conditions = @(Read-Rows $db 'SELECT `Condition` FROM `LaunchCondition`' @('Condition'))
    if (-not ($conditions | Where-Object { $_.Condition -match 'NOT LEGACY_PAYLOAD' -and $_.Condition -match 'NOT LEGACY_REGISTRATION' -and $_.Condition -match 'NOT LEGACY_UNINSTALL' })) { throw 'Bound MSI missing legacy ownership guard' }
    if (-not ($conditions | Where-Object { $_.Condition -eq 'Installed OR NOT SAME_VERSION_PRODUCT' })) { throw 'Bound MSI missing same-version guard' }
    $upgrades = @(Read-Rows $db 'SELECT `UpgradeCode`, `Attributes`, `ActionProperty` FROM `Upgrade`' @('Code','Flags','Property'))
    if (-not ($upgrades | Where-Object Property -eq 'WIX_DOWNGRADE_DETECTED')) { throw 'Downgrade guard missing' }
    $services = if ($tableNames -contains 'ServiceInstall') { @(Read-Rows $db 'SELECT `Name`, `Arguments`, `StartName`, `ErrorControl` FROM `ServiceInstall`' @('Name','Arguments','Account','Error')) } else { @() }
    if ($Product -eq 'customer-desk') {
        if ($services.Count -ne 1 -or $services[0].Name -ne $name -or $services[0].Arguments -ne '--service' -or $services[0].Account -ne 'LocalSystem' -or ([int]$services[0].Error -band 32768) -eq 0) { throw 'Wrong/non-vital service declaration' }
        $control = @(Read-Rows $db 'SELECT `Name`, `Event`, `Wait` FROM `ServiceControl`' @('Name','Events','Wait'))
        if ($control.Count -ne 1 -or $control[0].Name -ne $name -or $control[0].Wait -ne '1' -or [int]$control[0].Events -ne 163) { throw 'Wrong service lifecycle' }
    } elseif ($services.Count -ne 0 -or $tableNames -contains 'ServiceControl') { throw 'Console must not install services' }
    $registry = @(Read-Rows $db 'SELECT `Root`, `Key`, `Name`, `Value` FROM `Registry`' @('Root','Key','Name','Value'))
    $scope = if ($Product -eq 'customer-desk') { '2' } else { '1' }
    if ($registry | Where-Object Root -ne $scope) { throw 'Registry scope mismatch' }
    if ($Product -eq 'support-console') {
        $uri = if ($Probe) { 'ongrow-msi-probe-console' } else { 'ongrow-support-console' }
        if (-not ($registry | Where-Object { $_.Key -eq "Software\Classes\$uri\shell\open\command" -and $_.Value -eq "`"[INSTALLFOLDER]$name.exe`" `"%1`"" })) { throw 'Console URI is missing/wrong' }
    }
    # Exact source schema already rejects foreign targets; inspect bound native directory table too.
    $dirs = @(Read-Rows $db 'SELECT `Directory`, `Directory_Parent`, `DefaultDir` FROM `Directory`' @('Id','Parent','Name'))
    $install = $dirs | Where-Object Id -eq 'INSTALLFOLDER'
    $expectedParent = if ($Product -eq 'customer-desk') { 'ProgramFiles64Folder' } else { 'OnGrowFolder' }
    if ($install.Parent -ne $expectedParent) { throw 'Package install root mismatch' }
    if ($Probe -and (($dirs.Name -join '|') -match 'OnGROW Support (Desk|Console)')) { throw 'Probe contains customer paths' }
    return $props.ProductCode
}

function Invoke-Msi([string]$Verb, [string]$Target, [string]$Label, [int]$Expected = 0, [string[]]$Properties = @()) {
    if ($Verb -notin @('/i','/x')) { throw 'Unexpected MSI operation' }
    $log = Join-Path $root "$Label.log"
    $args = @($Verb, "`"$Target`"", '/qn', '/norestart', '/L*v', "`"$log`"") + $Properties
    $p = Start-Process -FilePath (Join-Path $env:WINDIR 'System32/msiexec.exe') -ArgumentList $args -PassThru
    if (-not $p.WaitForExit(180000)) { throw 'MSI timed out; do not kill a transaction or touch customer targets' }
    $p.Refresh()
    if ($p.ExitCode -ne $Expected) { throw "MSI $Label failed: exit $($p.ExitCode), expected $Expected (isolated log retained)" }
}

# Compile production schemas with synthetic inert bytes, then inspect tables only. NEVER install them.
foreach ($product in @('customer-desk','support-console')) {
    $src = Join-Path $root "schema-$product"
    & python -c 'import sys; from pathlib import Path; sys.path.insert(0,"scripts"); from test_ongrow_windows_msi import fixture; fixture(Path(sys.argv[1]),sys.argv[2])' $src $product
    if ($LASTEXITCODE -ne 0) { throw 'Synthetic schema fixture failed' }
    $out = Join-Path $root "schema-package-$product"
    & python scripts/package_ongrow_windows_msi.py --source $src --output $out --product $product --sequence 1 --upstream-version 1.4.9 --source-sha ('a' * 40)
    if ($LASTEXITCODE -ne 0) { throw 'Production schema compile failed' }
    $null = Assert-Tables (Join-Path $out 'bin/Release/OnGROW.msi') $product $false 1
}

foreach ($product in @('customer-desk','support-console')) {
    $name = if ($product -eq 'customer-desk') { 'OnGROW MSI Probe Desk' } else { 'OnGROW MSI Probe Console' }
    $install = if ($product -eq 'customer-desk') { Join-Path $env:ProgramFiles $name } else { Join-Path $env:LOCALAPPDATA 'Programs/OnGROW/MSI Probe Console' }
    $reg = if ($product -eq 'customer-desk') { 'HKLM:\Software\OnGROW\MSIProbe\customer-desk' } else { 'HKCU:\Software\OnGROW\MSIProbe\support-console' }
    if ((Test-Path $install) -or (Test-Path $reg) -or (Get-Service -Name $name -ErrorAction SilentlyContinue)) { throw 'Probe target pre-exists; refusing mutation' }
    $sentinel = Join-Path $root "$product-preserved-appdata.txt"
    [IO.File]::WriteAllText($sentinel, 'device-and-grant-sentinel')
    $packages = @{}; $codes = @{}
    for ($v = 1; $v -le 3; $v++) {
        $source = Join-Path $root "$product-v$v"
        $null = New-Item -ItemType Directory -Path $source
        $cs = Join-Path $root "$product-v$v.cs"
        [IO.File]::WriteAllText($cs, $fixture.Replace('@NAME@', $name).Replace('@VERSION@', "0.0.$v.0"))
        & $csc /nologo /target:exe /platform:x64 "/out:$(Join-Path $source "$name.exe")" /reference:System.ServiceProcess.dll $cs
        if ($LASTEXITCODE -ne 0) { throw 'Network-free fixture compile failed' }
        [IO.File]::WriteAllText((Join-Path $source 'probe-version.txt'), "$v")
        $output = Join-Path $root "$product-package-v$v"
        & python scripts/package_ongrow_windows_msi.py --source $source --output $output --product $product --sequence $v --upstream-version 1.4.9 --source-sha $sha --ci-probe
        if ($LASTEXITCODE -ne 0) { throw 'Probe MSI compile failed' }
        $packages[$v] = Join-Path $output 'bin/Release/OnGROW.msi'
        $codes[$v] = Assert-Tables $packages[$v] $product $true $v
    }
    $sameVersionOutput = Join-Path $root "$product-package-v2-other-identity"
    & python scripts/package_ongrow_windows_msi.py --source (Join-Path $root "$product-v2") --output $sameVersionOutput --product $product --sequence 2 --upstream-version 1.4.9 --source-sha ('b' * 40) --ci-probe
    if ($LASTEXITCODE -ne 0) { throw 'Same-version probe compile failed' }
    $sameVersionMsi = Join-Path $sameVersionOutput 'bin/Release/OnGROW.msi'
    $sameVersionCode = Assert-Tables $sameVersionMsi $product $true 2
    if ($sameVersionCode -eq $codes[2]) { throw 'Same-version test does not change package identity' }
    function Assert-ProbeVersion([int]$Version) {
        if ((Get-ItemProperty $reg).Sequence -ne "$Version" -or (Get-ItemProperty $reg).Version -ne "0.0.$Version") { throw 'Installed registry version mismatch' }
        if ([IO.File]::ReadAllText((Join-Path $install 'probe-version.txt')) -ne "$Version") { throw 'Installed data version mismatch' }
        if ((Get-Item (Join-Path $install "$name.exe")).VersionInfo.FileVersion -ne "0.0.$Version.0") { throw 'Installed executable version mismatch' }
        if ([IO.File]::ReadAllText($sentinel) -ne 'device-and-grant-sentinel') { throw 'External application data changed' }
        if ($product -eq 'customer-desk') {
            if ((Get-Service -Name $name).Status -ne 'Running') { throw 'Probe service not running after transaction' }
            if ((Get-ItemProperty $reg).RunningVersion -ne "0.0.$Version.0") { throw 'Loaded service version mismatch after upgrade/rollback' }
        }
        if ($product -eq 'support-console' -and (Get-Service -Name $name -ErrorAction SilentlyContinue)) { throw 'Console installed a service' }
    }
    $installed = $false
    try {
        Invoke-Msi '/i' $packages[1] "$product-install-v1"
        $installed = $true
        Assert-ProbeVersion 1
        Invoke-Msi '/i' $packages[2] "$product-upgrade-v2"
        Assert-ProbeVersion 2
        Invoke-Msi '/i' $sameVersionMsi "$product-reject-same-version" 1603
        Assert-ProbeVersion 2
        Invoke-Msi '/i' $packages[3] "$product-fail-v3" 1603 @('PROBE_FAIL=1')
        Assert-ProbeVersion 2
        Invoke-Msi '/i' $packages[1] "$product-downgrade-v1" 1603
        Assert-ProbeVersion 2
        Invoke-Msi '/x' $codes[2] "$product-uninstall-v2"
        $installed = $false
        if ((Test-Path (Join-Path $install "$name.exe")) -or (Test-Path $reg) -or (Get-Service -Name $name -ErrorAction SilentlyContinue)) { throw 'Probe uninstall incomplete' }
        if ([IO.File]::ReadAllText($sentinel) -ne 'device-and-grant-sentinel') { throw 'Uninstall touched app data' }
    } finally {
        if ($installed) {
            # Only these exact three probe ProductCodes can ever be cleaned up.
            foreach ($v in @(3,2,1)) {
                try { Invoke-Msi '/x' $codes[$v] "$product-cleanup-v$v" } catch { Write-Warning 'Probe cleanup failed or product absent; runner is disposable' }
            }
        }
    }
}
Write-Host 'Native MSI schema inspection and isolated install/upgrade/rollback/downgrade/uninstall passed for both products.'
