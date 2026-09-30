$ErrorActionPreference = 'Stop'
$owner = 'de.ongrow.supportconsole'
$scheme = 'HKCU:\Software\Classes\ongrow-support-console'
$uninstall = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\OnGROWSupportConsole'
$install = Join-Path $env:LOCALAPPDATA 'Programs\OnGROW\Support Console'
$exe = Join-Path $install 'OnGROW Support Console.exe'
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

Assert-NoRedirect $install

if (-not (Test-Path -LiteralPath $uninstall)) { throw 'Support Console ownership record missing' }
$record = Get-ItemProperty -LiteralPath $uninstall
if ($record.OnGROWOwner -ne $owner -or $record.InstallLocation -ne $install -or
    $record.UninstallString -ne ('powershell.exe -NoProfile -File "{0}"' -f (Join-Path $install 'uninstall_ongrow_windows_console.ps1'))) {
    throw 'Support Console ownership record does not match this installation'
}
if (Test-Path -LiteralPath $scheme) {
    $schemeOwner = (Get-ItemProperty -LiteralPath $scheme -Name OnGROWOwner -ErrorAction SilentlyContinue).OnGROWOwner
    $commandKey = Get-Item -LiteralPath (Join-Path $scheme 'shell\open\command') -ErrorAction SilentlyContinue
    if ($schemeOwner -ne $owner -or $null -eq $commandKey -or $commandKey.GetValue('') -ne ('"{0}" "%1"' -f $exe)) {
        throw 'URI association is not owned by this Support Console installation'
    }
}
if (Test-Path -LiteralPath $install) {
    if ((Get-Item -LiteralPath $install).Attributes -band [IO.FileAttributes]::ReparsePoint) {
        throw 'Refusing a redirected install directory'
    }
    if (-not (Test-Path -LiteralPath $manifest -PathType Leaf)) {
        throw 'Owned-file manifest missing'
    }
    if (Get-ChildItem -LiteralPath $install -Recurse -Force |
        Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint }) {
        throw 'Refusing redirected content in install directory'
    }
    $owned = @(Get-Content -LiteralPath $manifest)
    Assert-OwnedPaths $owned
    $actual = @(Get-ChildItem -LiteralPath $install -File -Recurse -Force |
        ForEach-Object { $_.FullName.Substring($install.Length + 1) })
    $difference = Compare-Object -ReferenceObject @($owned + '.ongrow-owned-files') -DifferenceObject $actual
    if ($difference) { throw 'Install directory contains unexpected files; refusing deletion' }
}
if (Test-Path -LiteralPath $scheme) {
    Remove-Item -LiteralPath $scheme -Recurse
}
Remove-Item -LiteralPath $uninstall -Recurse
if (Test-Path -LiteralPath $install) {
    Remove-Item -LiteralPath $install -Recurse -Force
}
