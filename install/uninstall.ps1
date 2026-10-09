<#
.SYNOPSIS
  The Terror Bats Framework 0.2.0-rc.1 — uninstaller (Windows).

.DESCRIPTION
  Removes exactly what install.ps1 owns: the installed terrorbats.exe
  (plus installed completions shipped beside it) and, only when it was
  added by the installer, the PATH entry. Never touches Bat files,
  evidence stores, repositories, receipts, or unrelated PATH entries.
#>
param(
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA 'Programs\TerrorBats\bin')
)

$ErrorActionPreference = 'Stop'
$removed = @()

$binary = Join-Path $InstallDir 'terrorbats.exe'
if (Test-Path $binary) {
    Remove-Item $binary -Force
    $removed += $binary
}

foreach ($asset in @('completions', 'terrorbats.1.txt')) {
    $path = Join-Path $InstallDir $asset
    if (Test-Path $path) {
        Remove-Item $path -Force -Recurse
        $removed += $path
    }
}

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($userPath -like "*$InstallDir*") {
    $entries = $userPath -split ';' | Where-Object { $_ -ne '' -and $_ -ne $InstallDir }
    [Environment]::SetEnvironmentVariable('Path', ($entries -join ';'), 'User')
    Write-Host "PATH CHANGED: removed '$InstallDir' from the USER Path environment variable."
} else {
    Write-Host "PATH unchanged: '$InstallDir' was not present in the USER Path."
}

if ((Test-Path $InstallDir) -and -not (Get-ChildItem $InstallDir -Force | Select-Object -First 1)) {
    Remove-Item $InstallDir -Force
    Write-Host "Removed now-empty directory '$InstallDir'."
}

if ($removed.Count -eq 0) {
    Write-Host "Nothing owned by the installer was found under '$InstallDir'."
} else {
    Write-Host "Removed:"
    $removed | ForEach-Object { Write-Host "  $_" }
}
Write-Host "Untouched by design: Bat files, evidence stores, repositories, receipts."
