<#
.SYNOPSIS
  The Terror Bats Framework 0.2.0-rc.1 — uninstaller (Windows).

.DESCRIPTION
  Removes exactly what install.ps1 owns: the installed terrorbats.exe
  (plus installed completions shipped beside it) and, only when the
  installer appended it (ownership marker .terrorbats-path-added present),
  the PATH entry. A pre-existing entry without the marker is preserved.
  Never touches Bat files, evidence stores, repositories, receipts, or
  unrelated PATH entries.
#>
param(
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA 'Programs\TerrorBats\bin'),
    [ValidateSet('User', 'Process')][string]$PathScope = 'User'
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

$marker = Join-Path $InstallDir '.terrorbats-path-added'
$scopePath = [Environment]::GetEnvironmentVariable('Path', $PathScope)
$scopeEntries = @($scopePath -split ';' | Where-Object { $_ -ne '' })
if ((Test-Path $marker) -and ($scopeEntries -contains $InstallDir)) {
    # Exact-entry removal: siblings such as TerrorBats-Other are untouched.
    $entries = @($scopeEntries | Where-Object { $_ -ne $InstallDir })
    [Environment]::SetEnvironmentVariable('Path', ($entries -join ';'), $PathScope)
    Write-Host "PATH CHANGED: removed installer-added '$InstallDir' from the $PathScope Path environment variable."
    Remove-Item $marker -Force
} elseif (Test-Path $marker) {
    Write-Host "PATH unchanged: ownership marker present but '$InstallDir' not on the $PathScope Path; removing the marker."
    Remove-Item $marker -Force
} else {
    Write-Host "PATH unchanged: no installer ownership marker; pre-existing entries preserved."
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
