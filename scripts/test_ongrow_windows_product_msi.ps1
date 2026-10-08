# Install real product artifacts only on disposable, network-blocked GitHub runners.
# This proves fresh MSI installation, not updater health or automatic replacement.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('customer-desk','support-console')][string]$Product,
    [Parameter(Mandatory)][string]$ArtifactDirectory,
    [Parameter(Mandatory)][ValidatePattern('^[0-9a-f]{40}$')][string]$SourceSha
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_OS -ne 'Windows') { throw 'Disposable Windows CI required' }
if (-not [Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Elevated disposable runner required' }
$name = if ($Product -eq 'customer-desk') { 'OnGROW Support Desk' } else { 'OnGROW Support Console' }
$install = if ($Product -eq 'customer-desk') { Join-Path $env:ProgramW6432 $name } else { Join-Path $env:LOCALAPPDATA 'Programs\OnGROW\Support Console' }
$packageRegistry = if ($Product -eq 'customer-desk') { 'HKLM:\Software\OnGROW\Packages\customer-desk' } else { 'HKCU:\Software\OnGROW\Packages\support-console' }
$exe = Join-Path $install "$name.exe"
$artifact = [IO.Path]::GetFullPath($ArtifactDirectory)
$runnerTemp = [IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\') + '\'
if (-not $artifact.StartsWith($runnerTemp, [StringComparison]::OrdinalIgnoreCase)) { throw 'Artifact must be under disposable runner temp' }

function Assert-NoRedirect([string]$Path) {
    $current = [IO.Path]::GetFullPath($Path)
    while ($current) {
        if ((Test-Path -LiteralPath $current) -and ((Get-Item -LiteralPath $current -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Redirected test path refused' }
        $current = [IO.Path]::GetDirectoryName($current)
    }
}
Assert-NoRedirect $artifact
Assert-NoRedirect $install
if ((Test-Path -LiteralPath $install) -or (Test-Path -LiteralPath $packageRegistry) -or (Get-Service -Name $name -ErrorAction SilentlyContinue)) { throw 'Product already exists on disposable runner' }
$files = @(Get-ChildItem -LiteralPath $artifact -File -Force)
$packages = @($files | Where-Object Extension -eq '.msi')
if ($packages.Count -ne 1 -or $files.Count -ne 3 -or @(Get-ChildItem -LiteralPath $artifact -Directory -Force).Count -ne 0) { throw 'Unexpected MSI artifact shape' }
foreach ($file in $files) { Assert-NoRedirect $file.FullName }
$msi = $packages[0].FullName
$descriptor = Get-Content -LiteralPath (Join-Path $artifact 'package.json') -Raw | ConvertFrom-Json
if ($descriptor.product -ne $Product -or $descriptor.probe_only -ne $false -or $descriptor.compiled -ne $true -or $descriptor.source_sha -ne $SourceSha) { throw 'MSI completion identity mismatch' }
$sidecar = (Get-Content -LiteralPath "$msi.sha256" -Raw).Trim()
if ($sidecar -notmatch '^([0-9a-f]{64})  ([a-zA-Z0-9_.-]+\.msi)$' -or $Matches[2] -ne $packages[0].Name) { throw 'Invalid MSI checksum sidecar' }
if ((Get-FileHash -LiteralPath $msi -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Matches[1]) { throw 'MSI checksum mismatch' }
$expectedCode = (& python -c 'import sys; sys.path.insert(0,"scripts"); import package_ongrow_windows_msi as m; print(m.identity(sys.argv[1], "release/" + sys.argv[2] + "/" + sys.argv[3]))' $Product $descriptor.sequence $SourceSha).Trim()
if ($LASTEXITCODE -ne 0) { throw 'Product identity calculation failed' }
$installer = New-Object -ComObject WindowsInstaller.Installer
$database = $installer.OpenDatabase($msi, 0)
$view = $database.OpenView('SELECT `Property`, `Value` FROM `Property`')
$properties = @{}
try {
    $null = $view.Execute()
    while ($null -ne ($row = $view.Fetch())) { $properties[$row.StringData(1)] = $row.StringData(2) }
} finally { $null = $view.Close() }
if ($properties.ProductName -ne $name -or $properties.Manufacturer -ne 'OnGROW GmbH' -or $properties.ProductVersion -ne $descriptor.msi_version -or $properties.ProductCode.Trim('{}') -ne $expectedCode) { throw 'Native MSI identity mismatch' }
$productCode = $properties.ProductCode
# Deny both directions before installing. SCM child processes use this exact
# installed executable too. Rules remain through runner teardown on failure.
$rulePrefix = "OnGROW disposable MSI $Product $env:GITHUB_RUN_ID"
foreach ($direction in @('Inbound','Outbound')) {
    $ruleName = "$rulePrefix $direction"
    if (Get-NetFirewallRule -DisplayName $ruleName -ErrorAction SilentlyContinue) { throw 'Test firewall rule already exists' }
    New-NetFirewallRule -DisplayName $ruleName -Direction $direction -Action Block -Program $exe -Profile Any | Out-Null
    $rule = Get-NetFirewallRule -DisplayName $ruleName
    $filter = $rule | Get-NetFirewallApplicationFilter
    if ($rule.Enabled -ne 'True' -or $rule.Action -ne 'Block' -or $filter.Program -ne $exe) { throw 'Product network isolation failed' }
}

function Invoke-Msi([string[]]$Arguments) {
    # Never kill a Windows Installer transaction or infer quiescence from timeout.
    $process = Start-Process -FilePath (Join-Path $env:WINDIR 'System32\msiexec.exe') -ArgumentList $Arguments -PassThru
    if (-not $process.WaitForExit(600000)) { throw 'Native MSI still running; retain runner state' }
    if ($process.ExitCode -ne 0) { throw "Native MSI failed with code $($process.ExitCode)" }
}
Invoke-Msi @('/i', ('"{0}"' -f $msi), '/qn', '/norestart')
$record = Get-ItemProperty -LiteralPath $packageRegistry
if ($record.Source -ne $SourceSha -or $record.Sequence -ne [string]$descriptor.sequence -or $record.Version -ne $descriptor.msi_version -or $record.InstallDirectory.TrimEnd('\') -ne $install) { throw 'Installed package registry mismatch' }
foreach ($entry in $descriptor.payload_sha256.PSObject.Properties) {
    $relative = $entry.Name
    if ($relative -match '(^|/)\.\.?(/|$)' -or $relative -match '[\\:\x00-\x1f]' -or $relative.StartsWith('/')) { throw 'Invalid descriptor payload path' }
    $path = [IO.Path]::GetFullPath((Join-Path $install $relative))
    if (-not $path.StartsWith($install + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Payload escaped install root' }
    Assert-NoRedirect $path
    if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() -ne $entry.Value) { throw 'Installed payload hash mismatch' }
}
$version = (Get-Item -LiteralPath $exe).VersionInfo
if ($version.CompanyName -ne 'OnGROW GmbH' -or $version.ProductName -ne $name) { throw 'Installed executable product mismatch' }
if ($Product -eq 'customer-desk') {
    $service = Get-CimInstance Win32_Service -Filter "Name='OnGROW Support Desk'"
    if ($service.State -ne 'Running' -or $service.StartName -ne 'LocalSystem' -or $service.PathName -ne ('"{0}" --service' -f $exe)) { throw 'Real Desk service installation/start failed' }
} else {
    if (Get-Service -Name $name -ErrorAction SilentlyContinue) { throw 'Console created a service' }
    $uriCommand = (Get-Item -LiteralPath 'HKCU:\Software\Classes\ongrow-support-console\shell\open\command').GetValue('')
    if ($uriCommand -ne ('"{0}" "%1"' -f $exe)) { throw 'Console URI points at wrong executable' }
}
Write-Output "NATIVE_REAL_PRODUCT_FRESH_INSTALL_PASS:$Product"
# Uninstall only the exact, checked ProductCode installed above. No recursive
# cleanup or user-profile/config reset. Unknown install failures do not get here.
Invoke-Msi @('/x', $productCode, '/qn', '/norestart')
if ((Test-Path -LiteralPath $exe) -or (Test-Path -LiteralPath $packageRegistry) -or (Get-Service -Name $name -ErrorAction SilentlyContinue)) { throw 'Owned MSI uninstall incomplete' }
if ($Product -eq 'support-console' -and (Test-Path -LiteralPath 'HKCU:\Software\Classes\ongrow-support-console\shell\open\command')) { throw 'Owned Console URI remained after uninstall' }
Write-Output "NATIVE_REAL_PRODUCT_UNINSTALL_PASS:$Product"
