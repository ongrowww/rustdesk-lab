$ErrorActionPreference = 'Stop'
foreach ($name in @('install_ongrow_windows_console.ps1', 'uninstall_ongrow_windows_console.ps1', 'test_ongrow_windows_console_install.ps1')) {
    $path = Join-Path $PSScriptRoot $name
    $tokens = $null
    $errors = $null
    [System.Management.Automation.Language.Parser]::ParseFile(
        $path, [ref]$tokens, [ref]$errors) | Out-Null
    if ($errors.Count -gt 0) {
        throw "Invalid PowerShell syntax in $name"
    }
}
