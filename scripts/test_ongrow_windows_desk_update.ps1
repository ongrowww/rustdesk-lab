# Real product update on a disposable Windows runner, not an installer probe.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$BaselineDirectory,
    [Parameter(Mandatory)][string]$CandidateDirectory,
    [Parameter(Mandatory)][ValidatePattern('^[0-9a-f]{40}$')][string]$BaselineSha,
    [Parameter(Mandatory)][ValidatePattern('^[0-9a-f]{40}$')][string]$CandidateSha
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_OS -ne 'Windows') { throw 'Disposable Windows runner required' }
$install = Join-Path $env:ProgramW6432 'OnGROW Support Desk'
$exe = Join-Path $install 'OnGROW Support Desk.exe'
$state = Join-Path $env:ProgramData 'OnGROW\Support Desk Update'
$registry = 'HKLM:\Software\OnGROW\Packages\customer-desk'
if ((Test-Path $install) -or (Test-Path $state) -or (Test-Path $registry) -or (Get-Service 'OnGROW Support Desk' -ErrorAction SilentlyContinue)) { throw 'Fresh real-product fixture required' }

function Assert-NoRedirect([string]$Path) {
    $p = [IO.Path]::GetFullPath($Path)
    while ($p) {
        if ((Test-Path -LiteralPath $p) -and ((Get-Item -LiteralPath $p -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Redirected product fixture refused' }
        $p = [IO.Path]::GetDirectoryName($p)
    }
}
function Read-Artifact([string]$Directory, [string]$Sha) {
    $dir = [IO.Path]::GetFullPath($Directory)
    if (-not $dir.StartsWith([IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Artifact must be in runner temporary storage' }
    Assert-NoRedirect $dir
    $files = @(Get-ChildItem -LiteralPath $dir -File -Force)
    $packages = @($files | Where-Object Extension -eq '.msi')
    if ($files.Count -ne 3 -or $packages.Count -ne 1 -or @(Get-ChildItem -LiteralPath $dir -Directory).Count -ne 0) { throw 'Unexpected product artifact shape' }
    foreach ($file in $files) { Assert-NoRedirect $file.FullName }
    $descriptor = Get-Content -LiteralPath (Join-Path $dir 'package.json') -Raw | ConvertFrom-Json
    if ($descriptor.product -ne 'customer-desk' -or $descriptor.probe_only -ne $false -or $descriptor.compiled -ne $true -or $descriptor.source_sha -ne $Sha) { throw 'Product build identity mismatch' }
    $checksum = (Get-Content -LiteralPath ($packages[0].FullName + '.sha256') -Raw).Trim()
    if ($checksum -notmatch '^([0-9a-f]{64})  ([A-Za-z0-9_.-]+\.msi)$' -or $Matches[2] -ne $packages[0].Name -or (Get-FileHash -LiteralPath $packages[0].FullName -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Matches[1]) { throw 'Product artifact checksum mismatch' }
    return @{ Package = $packages[0].FullName; Descriptor = $descriptor }
}
$baseline = Read-Artifact $BaselineDirectory $BaselineSha
$candidate = Read-Artifact $CandidateDirectory $CandidateSha
if ([UInt64]$candidate.Descriptor.sequence -le [UInt64]$baseline.Descriptor.sequence) { throw 'Increasing actual package sequence required' }
if (@(Get-NetFirewallProfile | Where-Object Enabled -ne $true).Count -ne 0 -or (Get-Service MpsSvc).Status -ne 'Running') { throw 'Existing runner firewall must enforce isolation' }
foreach ($direction in @('Inbound', 'Outbound')) {
    New-NetFirewallRule -DisplayName ('OnGROW actual updater isolation ' + $direction) -Direction $direction -Action Block -Program $exe -Profile Any | Out-Null
}
# The Desk GUI/service/server cannot register a device or contact peers. Only
# the separate updater may fetch its fixed public signed manifest and payload.
$native = Start-Process -FilePath (Join-Path $env:WINDIR 'System32\msiexec.exe') -ArgumentList @('/i', ('"' + $baseline.Package + '"'), '/qn', '/norestart') -PassThru
if (-not $native.WaitForExit(600000) -or $native.ExitCode -ne 0) { throw 'Baseline installation failed; retain runner state' }
$old = Get-ItemProperty -LiteralPath $registry
if ($old.Sequence -ne [string]$baseline.Descriptor.sequence -or $old.Source -ne $BaselineSha) { throw 'Baseline installed identity mismatch' }
$gui = Start-Process -FilePath $exe -ArgumentList '--no-server' -PassThru
# This harness does not invoke the candidate MSI. The actual service must
# download, verify, quiesce, install, check loaded health and commit itself.
$deadline = [DateTime]::UtcNow.AddMinutes(12)
$updated = $false
while ([DateTime]::UtcNow -lt $deadline) {
    Start-Sleep -Seconds 2
    $now = Get-ItemProperty -LiteralPath $registry -ErrorAction SilentlyContinue
    if ($null -ne $now -and $now.Sequence -eq [string]$candidate.Descriptor.sequence -and $now.Source -eq $CandidateSha) {
        $journal = Join-Path $state 'state-v1.journal'
        $accepted = Join-Path $state 'accepted-sequence-v1'
        if ((Test-Path $journal) -and (Test-Path $accepted)) {
            $j = [IO.File]::ReadAllBytes($journal)
            $a = [IO.File]::ReadAllBytes($accepted)
            if ($j.Length -eq 1 -and $j[0] -eq 0 -and $a.Length -eq 56) {
                $bytes = $a[16..23]; [Array]::Reverse($bytes)
                if ([BitConverter]::ToUInt64($bytes, 0) -eq [UInt64]$candidate.Descriptor.sequence) { $updated = $true; break }
            }
        }
    }
}
if (-not $updated) {
    foreach ($name in @('admission-v1.lock', 'state-v1.journal', 'accepted-sequence-v1', 'stage-v1.payload', 'attempt-v1.signed', 'native-attempt-v1')) {
        Write-Output ('UPDATE_FIXTURE_FILE_' + $name + '=' + [bool](Test-Path (Join-Path $state $name)))
    }
    throw 'Actual scheduled Desk update did not complete; no forced transaction termination or cleanup'
}
if (-not $gui.HasExited) { throw 'Old GUI survived transactional replacement' }
$reopened=$false
$guiDeadline=[DateTime]::UtcNow.AddSeconds(30)
do {
    $windows=@(Get-CimInstance Win32_Process -Filter "Name='OnGROW Support Desk.exe'" | Where-Object { $_.ExecutablePath -eq $exe -and $_.CommandLine -notmatch '--(service|server|tray|cm)' })
    if ($windows.Count -gt 0) { $reopened=$true;break }
    Start-Sleep -Seconds 1
} while ([DateTime]::UtcNow -lt $guiDeadline)
if (-not $reopened) { throw 'Updated user GUI did not reopen after Ready' }

foreach ($entry in $candidate.Descriptor.payload_sha256.PSObject.Properties) {
    $path = Join-Path $install $entry.Name
    Assert-NoRedirect $path
    if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() -ne $entry.Value) { throw 'Installed candidate payload mismatch' }
}
$service = Get-CimInstance Win32_Service -Filter "Name='OnGROW Support Desk'"
if ($service.State -ne 'Running' -or $service.StartName -ne 'LocalSystem') { throw 'Replacement service not healthy' }
$health = [IO.File]::ReadAllBytes((Join-Path $state 'service-health-v1'))
if ($health.Length -ne 68) { throw 'Replacement loaded health missing' }
$bytes = $health[8..15]; [Array]::Reverse($bytes)
$pidBytes = $health[56..59]; [Array]::Reverse($pidBytes)
if ([BitConverter]::ToUInt64($bytes, 0) -ne [UInt64]$candidate.Descriptor.sequence -or [Text.Encoding]::ASCII.GetString($health, 16, 40) -ne $CandidateSha -or [BitConverter]::ToUInt32($pidBytes, 0) -ne $service.ProcessId) { throw 'Loaded replacement identity mismatch' }
foreach ($name in @('stage-v1.payload', 'attempt-v1.signed', 'installer-v1.msi', 'native-attempt-v1', 'native-attempt-v1.next')) {
    if (Test-Path (Join-Path $state $name)) { throw 'Committed update retained an active attempt slot' }
}
Write-Output 'ACTUAL_SCHEDULED_SIGNED_DESK_TO_DESK_UPDATE_PASS'
# Leave the replaced product intact until disposable runner destruction.
