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

if (-not (Test-Path -LiteralPath (Join-Path $source 'OnGROW Support Console.exe') -PathType Leaf) -or
    -not (Test-Path -LiteralPath (Join-Path $source 'librustdesk.dll') -PathType Leaf) -or
    -not (Test-Path -LiteralPath (Join-Path $source 'data') -PathType Container)) {
    throw 'Incomplete Support Console artifact'
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
$relativeFiles = @($sourceFiles | ForEach-Object { $_.FullName.Substring($source.Length + 1) })
if ($relativeFiles -contains '.ongrow-owned-files') { throw 'Artifact contains reserved manifest name' }
$oldFiles = @()
if (Test-Path -LiteralPath $install) {
    if (-not (Test-Path -LiteralPath $manifest)) { throw 'Existing install has no owned-file manifest' }
    $oldFiles = @(Get-Content -LiteralPath $manifest)
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
Get-ChildItem -LiteralPath $source | Copy-Item -Destination $install -Recurse -Force
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
