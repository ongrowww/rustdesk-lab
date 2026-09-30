param(
    [Parameter(Mandatory = $true)][string]$SourceDirectory
)

$ErrorActionPreference = 'Stop'
$owner = 'de.ongrow.supportconsole'
$scheme = 'HKCU:\Software\Classes\ongrow-support-console'
$uninstall = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\OnGROWSupportConsole'
$install = Join-Path $env:LOCALAPPDATA 'Programs\OnGROW\Support Console'
$exe = Join-Path $install 'OnGROW Support Console.exe'
$source = (Resolve-Path -LiteralPath $SourceDirectory).Path
$manifest = Join-Path $install '.ongrow-owned-files'

function Assert-NoRedirect([string]$Path) {
    $current = [IO.Path]::GetFullPath($Path)
    while ($current) {
        if ((Test-Path -LiteralPath $current) -and
            ((Get-Item -LiteralPath $current -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw 'Refusing a redirected path or parent directory'
        }
        $current = [IO.Path]::GetDirectoryName($current)
    }
}

function Assert-OwnedPaths($Paths) {
    foreach ($relative in $Paths) {
        if ([string]::IsNullOrWhiteSpace($relative) -or [IO.Path]::IsPathRooted($relative) -or
            $relative -match '[<>:"|?*\x00-\x1f]' -or
            $relative -match '(^|[\\/])([.]+|[^\\/]*[. ])([\\/]|$)' -or
            $relative -match '[\\/]{2}' -or $relative -eq '.ongrow-owned-files' -or
            -not [IO.Path]::GetFullPath((Join-Path $install $relative)).StartsWith(
                $install + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
            throw 'Owned-file manifest contains an invalid path'
        }
    }
}

Assert-NoRedirect $source
Assert-NoRedirect $install
$sourcePrefix = $source.TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
if ($sourcePrefix -eq ($install + [IO.Path]::DirectorySeparatorChar) -or
    $sourcePrefix.StartsWith($install + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) -or
    ($install + [IO.Path]::DirectorySeparatorChar).StartsWith($sourcePrefix, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Artifact and install directory must not overlap'
}

if (-not (Test-Path -LiteralPath (Join-Path $source 'OnGROW Support Console.exe') -PathType Leaf) -or
    -not (Test-Path -LiteralPath (Join-Path $source 'librustdesk.dll') -PathType Leaf) -or
    -not (Test-Path -LiteralPath (Join-Path $source 'data') -PathType Container) -or
    -not (Test-Path -LiteralPath (Join-Path $source 'uninstall_ongrow_windows_console.ps1') -PathType Leaf)) {
    throw 'Incomplete Support Console artifact'
}
if (Test-Path -LiteralPath $uninstall) {
    $record = Get-ItemProperty -LiteralPath $uninstall
    if ($record.InstallLocation -ne $install -or
        $record.UninstallString -ne ('powershell.exe -NoProfile -File "{0}"' -f (Join-Path $install 'uninstall_ongrow_windows_console.ps1'))) {
        throw 'Support Console ownership record does not match this installation'
    }
}
if (Test-Path -LiteralPath $scheme) {
    $existingCommand = (Get-Item -LiteralPath (Join-Path $scheme 'shell\open\command') -ErrorAction SilentlyContinue)
    if ($null -eq $existingCommand -or $existingCommand.GetValue('') -ne ('"{0}" "%1"' -f $exe)) {
        throw 'URI association is not owned by this Support Console installation'
    }
}
foreach ($key in @($scheme, $uninstall)) {
    if (Test-Path -LiteralPath $key) {
        $existingOwner = (Get-ItemProperty -LiteralPath $key -Name OnGROWOwner -ErrorAction SilentlyContinue).OnGROWOwner
        if ($existingOwner -ne $owner) { throw "Registry entry is not owned by Support Console: $key" }
    }
}
if ((Test-Path -LiteralPath $install) -and -not (Test-Path -LiteralPath $uninstall)) {
    throw 'Existing install directory has no Support Console ownership record'
}
if ((Test-Path -LiteralPath $install) -and
    ((Get-Item -LiteralPath $install).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
    throw 'Refusing a redirected install directory'
}
if (Get-ChildItem -LiteralPath $source -Recurse -Force |
    Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint }) {
    throw 'Artifact contains a redirected path'
}
$sourceFiles = Get-ChildItem -LiteralPath $source -File -Recurse -Force
$relativeFiles = @($sourceFiles | ForEach-Object { $_.FullName.Substring($sourcePrefix.Length) })
if ($relativeFiles -contains '.ongrow-owned-files') { throw 'Artifact contains reserved manifest name' }
Assert-OwnedPaths $relativeFiles
$oldFiles = @()
if (Test-Path -LiteralPath $install) {
    if (-not (Test-Path -LiteralPath $manifest -PathType Leaf)) { throw 'Existing install has no owned-file manifest' }
    $oldFiles = @(Get-Content -LiteralPath $manifest)
    Assert-OwnedPaths $oldFiles
    if (Get-ChildItem -LiteralPath $install -Recurse -Force |
        Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint }) {
        throw 'Existing install contains a redirected path'
    }
    $actual = @(Get-ChildItem -LiteralPath $install -File -Recurse -Force |
        ForEach-Object { $_.FullName.Substring($install.Length + 1) })
    $difference = Compare-Object -ReferenceObject @($oldFiles + '.ongrow-owned-files') -DifferenceObject $actual
    if ($difference) { throw 'Existing install contains unexpected files' }
}

New-Item -ItemType Directory -Force -Path $install | Out-Null
Get-ChildItem -LiteralPath $source -Force | Copy-Item -Destination $install -Recurse -Force
@($oldFiles + $relativeFiles | Sort-Object -Unique) | Set-Content -LiteralPath $manifest -Encoding UTF8
New-Item -Path $scheme -Force | Out-Null
New-ItemProperty -Path $scheme -Name OnGROWOwner -Value $owner -PropertyType String -Force | Out-Null
New-ItemProperty -Path $scheme -Name 'URL Protocol' -Value '' -PropertyType String -Force | Out-Null
New-Item -Path (Join-Path $scheme 'shell\open\command') -Force | Out-Null
$command = '"{0}" "%1"' -f $exe
Set-Item -Path (Join-Path $scheme 'shell\open\command') -Value $command
New-Item -Path $uninstall -Force | Out-Null
New-ItemProperty -Path $uninstall -Name OnGROWOwner -Value $owner -PropertyType String -Force | Out-Null
New-ItemProperty -Path $uninstall -Name DisplayName -Value 'OnGROW Support Console' -PropertyType String -Force | Out-Null
New-ItemProperty -Path $uninstall -Name InstallLocation -Value $install -PropertyType String -Force | Out-Null
$uninstallCommand = 'powershell.exe -NoProfile -File "{0}"' -f (Join-Path $install 'uninstall_ongrow_windows_console.ps1')
New-ItemProperty -Path $uninstall -Name UninstallString -Value $uninstallCommand -PropertyType String -Force | Out-Null
