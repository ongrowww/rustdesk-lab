# Functional installer tests. All HKCU operations are mocked, all files are temporary.
$ErrorActionPreference = 'Stop'
$testTemp = [IO.Path]::GetTempPath()
if ($IsMacOS -and $testTemp.StartsWith('/var/')) { $testTemp = '/private' + $testTemp }
$testRoot = Join-Path $testTemp ('ongrow-console-install-test-' + [Guid]::NewGuid())
Microsoft.PowerShell.Management\New-Item -ItemType Directory -Path $testRoot | Out-Null
$savedLocalAppData = $env:LOCALAPPDATA
$env:LOCALAPPDATA = Join-Path $testRoot 'user'
$registry = @{}
$redirectedPath = ''
$createdDrive = $false
if (-not (Get-PSDrive HKCU -ErrorAction SilentlyContinue)) {
    New-PSDrive -Name HKCU -PSProvider FileSystem -Root $testRoot | Out-Null
    $createdDrive = $true
}

function Registry-Key([string]$Path) { $Path.Replace('/', '\').TrimEnd('\') }
function Is-Registry([string]$Path) { $Path -like 'HKCU:*' }
function Test-Path {
    [CmdletBinding()]param([string]$LiteralPath, [string]$PathType)
    if (Is-Registry $LiteralPath) { return $registry.ContainsKey((Registry-Key $LiteralPath)) }
    if ($PathType) { return Microsoft.PowerShell.Management\Test-Path -LiteralPath $LiteralPath -PathType $PathType }
    Microsoft.PowerShell.Management\Test-Path -LiteralPath $LiteralPath
}
function Get-Item {
    [CmdletBinding()]param([string]$LiteralPath, [switch]$Force)
    if (Is-Registry $LiteralPath) {
        $key = Registry-Key $LiteralPath
        if (-not $registry.ContainsKey($key)) { return $null }
        $item = [PSCustomObject]@{ DefaultValue = $registry[$key][''] }
        $item | Add-Member -MemberType ScriptMethod -Name GetValue -Value { param($name) $this.DefaultValue }
        return $item
    }
    if ($LiteralPath -eq $redirectedPath) { return [PSCustomObject]@{ Attributes = [IO.FileAttributes]::ReparsePoint } }
    Microsoft.PowerShell.Management\Get-Item -LiteralPath $LiteralPath -Force:$Force
}
function Get-ItemProperty {
    [CmdletBinding()]param([string]$LiteralPath, [string]$Name)
    $key = Registry-Key $LiteralPath
    if (-not (Is-Registry $LiteralPath)) { throw 'Tests must not read real registry properties' }
    if (-not $registry.ContainsKey($key)) { return $null }
    [PSCustomObject]$registry[$key]
}
function New-Item {
    [CmdletBinding()]param([string]$Path, [string]$ItemType, [switch]$Force)
    if (Is-Registry $Path) {
        $key = Registry-Key $Path
        if (-not $registry.ContainsKey($key)) { $registry[$key] = @{} }
        return
    }
    Microsoft.PowerShell.Management\New-Item -Path $Path -ItemType $ItemType -Force:$Force
}
function New-ItemProperty {
    [CmdletBinding()]param([string]$Path, [string]$Name, $Value, [string]$PropertyType, [switch]$Force)
    if (-not (Is-Registry $Path)) { throw 'Tests must not write real registry properties' }
    $registry[(Registry-Key $Path)][$Name] = $Value
}
function Set-Item {
    [CmdletBinding()]param([string]$Path, $Value)
    if (-not (Is-Registry $Path)) { throw 'Tests must not set real registry values' }
    $registry[(Registry-Key $Path)][''] = $Value
}
function Remove-Item {
    [CmdletBinding()]param([string]$LiteralPath, [switch]$Recurse, [switch]$Force)
    if (Is-Registry $LiteralPath) {
        $key = Registry-Key $LiteralPath
        foreach ($candidate in @($registry.Keys)) {
            if ($candidate -eq $key -or $candidate.StartsWith($key + '\')) { $registry.Remove($candidate) }
        }
        return
    }
    if (-not [IO.Path]::GetFullPath($LiteralPath).StartsWith($testRoot + [IO.Path]::DirectorySeparatorChar)) {
        throw 'Test deletion escaped the temporary directory'
    }
    Microsoft.PowerShell.Management\Remove-Item -LiteralPath $LiteralPath -Recurse:$Recurse -Force:$Force
}
function Assert([bool]$Condition, [string]$Message) { if (-not $Condition) { throw $Message } }
function Assert-Fails([scriptblock]$Action, [string]$Expected) {
    $caught = $false
    try { & $Action } catch {
        $caught = $true
        if ($_.Exception.Message -notlike ('*' + $Expected + '*')) { throw }
    }
    Assert $caught ('Expected rejection: ' + $Expected)
}

try {
    $source = Join-Path $testRoot 'artifact'
    New-Item -ItemType Directory -Path (Join-Path $source 'data') -Force | Out-Null
    foreach ($relative in @('OnGROW Support Console.exe', 'librustdesk.dll', 'data/flutter_assets', '.hidden')) {
        Set-Content -LiteralPath (Join-Path $source $relative) -Value 'fixture'
    }
    $installer = Join-Path $PSScriptRoot 'install_ongrow_windows_console.ps1'
    $uninstaller = Join-Path $PSScriptRoot 'uninstall_ongrow_windows_console.ps1'
    Assert-Fails { & $installer -SourceDirectory $source } 'Incomplete Support Console artifact'
    Copy-Item -LiteralPath $uninstaller -Destination $source
    & $installer -SourceDirectory ($source + [IO.Path]::DirectorySeparatorChar)
    $install = Join-Path $env:LOCALAPPDATA 'Programs/OnGROW/Support Console'
    $manifest = Join-Path $install '.ongrow-owned-files'
    Assert (Test-Path -LiteralPath (Join-Path $install '.hidden')) 'Hidden source file was not copied'
    $owned = @(Get-Content -LiteralPath $manifest)
    Assert ($owned -contains '.hidden') 'Hidden source file missing from manifest'
    Assert ($owned -contains 'librustdesk.dll') 'Trailing source separator broke relative paths'
    $scheme = Registry-Key 'HKCU:\Software\Classes\ongrow-support-console'
    $uninstall = Registry-Key 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\OnGROWSupportConsole'
    $command = Registry-Key ($scheme + '\shell\open\command')
    $location = $registry[$uninstall].InstallLocation
    $registry[$uninstall].InstallLocation = 'foreign-location'
    Assert-Fails { & $installer -SourceDirectory $source } 'ownership record does not match'
    $registry[$uninstall].InstallLocation = $location
    $originalCommand = $registry[$command]['']
    $registry[$command][''] = 'foreign-command'
    Assert-Fails { & $installer -SourceDirectory $source } 'URI association is not owned'
    Assert-Fails { & $uninstaller } 'URI association is not owned'
    $registry[$command][''] = $originalCommand
    $originalUninstall = $registry[$uninstall].UninstallString
    $registry[$uninstall].UninstallString = 'foreign-uninstaller'
    Assert-Fails { & $installer -SourceDirectory $source } 'ownership record does not match'
    Assert-Fails { & $uninstaller } 'ownership record does not match'
    $registry[$uninstall].UninstallString = $originalUninstall
    foreach ($bad in @('../outside', '..\outside', '/absolute', 'C:\outside', 'file:stream', 'data/..', 'data/file.', 'data/file ', 'data//file', '')) {
        @($owned + $bad) | Set-Content -LiteralPath $manifest
        Assert-Fails { & $installer -SourceDirectory $source } 'invalid path'
        Assert-Fails { & $uninstaller } 'invalid path'
        Assert (Test-Path -LiteralPath (Join-Path $install 'OnGROW Support Console.exe')) 'Rejected manifest changed install files'
    }
    $owned | Set-Content -LiteralPath $manifest
    $redirectedPath = [IO.Path]::GetDirectoryName($install)
    Assert-Fails { & $installer -SourceDirectory $source } 'redirected path or parent'
    Assert-Fails { & $uninstaller } 'redirected path or parent'
    $redirectedPath = ''
    Set-Content -LiteralPath (Join-Path $install 'unexpected') -Value 'not-owned'
    Assert-Fails { & $installer -SourceDirectory $source } 'unexpected files'
    Assert-Fails { & $uninstaller } 'unexpected files'
    Remove-Item -LiteralPath (Join-Path $install 'unexpected')
    & $installer -SourceDirectory $source
    & $uninstaller
    Assert (-not (Test-Path -LiteralPath $install)) 'Owned install was not removed'
    Assert ($registry.Count -eq 0) 'Owned registry entries were not removed'
    Write-Output 'Installer tests passed with mocked HKCU and temporary artifact files'
} finally {
    $env:LOCALAPPDATA = $savedLocalAppData
    if ($createdDrive) { Remove-PSDrive HKCU }
    Microsoft.PowerShell.Management\Remove-Item -LiteralPath $testRoot -Recurse -Force
}
