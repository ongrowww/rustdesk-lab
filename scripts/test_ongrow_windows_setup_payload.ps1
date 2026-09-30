param(
    [Parameter(Mandatory = $true)][string]$SetupDirectory,
    [Parameter(Mandatory = $true)][string]$SourceSha,
    [Parameter(Mandatory = $true)][string]$Version,
    [int]$TimeoutSeconds = 60
)

$ErrorActionPreference = "Stop"
if (-not $IsWindows) { throw "Actual Setup payload verification requires Windows" }
if ($env:ONGROW_CI_SMOKE_TEST -ne "1") { throw "Offline CI isolation must be explicitly enabled" }
if ($SourceSha -cnotmatch '^[0-9a-f]{40}$') { throw "Exact source SHA required" }
if ($TimeoutSeconds -lt 1 -or $TimeoutSeconds -gt 120) { throw "Invalid verification timeout" }

$expectedName = "ongrow-support-desk-$Version-windows-x64-$($SourceSha.Substring(0, 8))-Setup.exe"
$setup = Join-Path (Resolve-Path $SetupDirectory) $expectedName
if (-not (Test-Path $setup -PathType Leaf)) { throw "Setup executable missing" }
$metadata = Get-Content "$setup.json" -Raw | ConvertFrom-Json
if ($metadata.product -cne "customer-desk" -or $metadata.platform -cne "windows-x64") { throw "Wrong Setup product/platform" }
if ($metadata.source_sha -cne $SourceSha -or $metadata.upstream_version -cne $Version) { throw "Setup provenance mismatch" }
if ($metadata.filename -cne $expectedName -or $metadata.size -ne (Get-Item $setup).Length) { throw "Setup filename/size mismatch" }
if ($metadata.unsigned_lab -ne $true) { throw "Setup must be labelled unsigned lab" }
$digest = (Get-FileHash $setup -Algorithm SHA256).Hash.ToLowerInvariant()
if ($metadata.sha256 -cne $digest) { throw "Setup metadata checksum mismatch" }
if ((Get-Content "$setup.sha256" -Raw).Trim() -cne "$digest  $expectedName") { throw "Setup checksum record mismatch" }

$bytes = [IO.File]::ReadAllBytes($setup)
if ($bytes.Length -lt 64 -or $bytes[0] -ne 0x4d -or $bytes[1] -ne 0x5a) { throw "Expected PE Setup" }
$peOffset = [BitConverter]::ToUInt32($bytes, 0x3c)
if ($peOffset + 6 -gt $bytes.Length -or [BitConverter]::ToUInt32($bytes, $peOffset) -ne 0x4550) { throw "Invalid Setup PE header" }
if ([BitConverter]::ToUInt16($bytes, $peOffset + 4) -ne 0x8664) { throw "Expected Setup AMD64" }
$identity = (Get-Item $setup).VersionInfo
if ($identity.CompanyName -cne "OnGROW GmbH" -or $identity.ProductName -cne "OnGROW Support Desk") { throw "Wrong Setup PE identity" }
if ($identity.InternalName -cne "ongrow_support_desk_setup" -or $identity.OriginalFilename -cne "OnGROW Support Desk Setup.exe") { throw "Wrong Setup PE filename contract" }
if ((Get-AuthenticodeSignature $setup).Status -ne "NotSigned") { throw "Lab Setup must remain unsigned" }

$cachePrefix = "OnGROW-Support-Desk-Setup-"
$before = @(Get-ChildItem $env:LOCALAPPDATA -Directory -Filter "$cachePrefix*" | Select-Object -ExpandProperty Name)
$temporary = Join-Path ([IO.Path]::GetTempPath()) ("ongrow-setup-offline-" + [Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $temporary | Out-Null
$knownFiles = @()
try {
    # Verify exactly the uploaded executable, then identical bytes under a different download name.
    $renamed = Join-Path $temporary "renamed-download.exe"
    Copy-Item $setup $renamed
    $knownFiles += $renamed
    foreach ($candidate in @($setup, $renamed)) {
        $stdout = Join-Path $temporary ([IO.Path]::GetRandomFileName())
        $stderr = Join-Path $temporary ([IO.Path]::GetRandomFileName())
        $knownFiles += @($stdout, $stderr)
        $process = Start-Process -FilePath $candidate -ArgumentList "--ongrow-verify-payload" `
            -RedirectStandardOutput $stdout -RedirectStandardError $stderr -PassThru
        $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
        try {
            while (-not $process.WaitForExit(100)) {
                $children = @(Get-CimInstance Win32_Process -Filter "ParentProcessId = $($process.Id)")
                if ($children.Count -gt 0) { throw "Offline payload check unexpectedly started a child" }
                if ([DateTime]::UtcNow -ge $deadline) { throw "Setup payload verification timed out" }
            }
            $process.WaitForExit()
            if ($process.ExitCode -ne 0) { throw "Setup payload verification exited unsuccessfully" }
            $output = Get-Content $stdout -Raw
            if ($output -notmatch 'Setup payload extracted and verified; [0-9]+ files; no client started') {
                throw "Setup did not confirm extraction and verification"
            }
            if (-not [string]::IsNullOrWhiteSpace((Get-Content $stderr -Raw))) { throw "Setup verification reported an error" }
        } finally {
            if (-not $process.HasExited) {
                # Only this invocation and its descendants, never any named host process.
                & taskkill /PID $process.Id /T /F | Out-Null
                if ($LASTEXITCODE -ne 0) { throw "Could not stop timed-out Setup invocation" }
            }
            $process.Dispose()
        }
    }
    $after = @(Get-ChildItem $env:LOCALAPPDATA -Directory -Filter "$cachePrefix*" | Select-Object -ExpandProperty Name)
    if (@($after | Where-Object { $_ -notin $before }).Count -gt 0) { throw "Setup left an unverified cache; retained for diagnosis" }
    if ((Get-FileHash $setup -Algorithm SHA256).Hash.ToLowerInvariant() -cne $digest) { throw "Final Setup checksum changed" }
    Write-Output "Actual unsigned AMD64 OnGROW Setup payload verified offline; no installation performed"
} finally {
    foreach ($known in $knownFiles) {
        if (Test-Path $known -PathType Leaf) { Remove-Item -LiteralPath $known }
    }
    # No recursive removal. Unknown entries remain and fail the test.
    if (Test-Path $temporary -PathType Container) { Remove-Item -LiteralPath $temporary }
}
