<#
.SYNOPSIS
  The Terror Bats Framework 0.2.0-rc.1 — uninstaller (Windows).

.DESCRIPTION
  Removes exactly what install.ps1 owns: the installed terrorbats.exe
  (plus installed completions shipped beside it) and, only when the
  installer appended it in the same scope (ownership marker content
  scope=<scope> matches), the PATH entry. A pre-existing entry, an entry
  owned in another scope, or unrecognised marker content never authorises
  removal. Never touches Bat files, evidence stores, repositories,
  receipts, or unrelated PATH entries.
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
$owner = ''
if (Test-Path $marker) {
    $owner = (Get-Content -Raw $marker).Trim()
}
if (($owner -ceq "scope=$PathScope") -and ($scopeEntries -contains $InstallDir)) {
    # Exact-entry removal that preserves unrelated separators: only elements
    # equal to InstallDir are dropped, so a pre-install PATH of 'A;;B;'
    # round-trips byte-for-byte and siblings such as TerrorBats-Other survive.
    $kept = @($scopePath -split ';' | Where-Object { $_ -ne $InstallDir })
    [Environment]::SetEnvironmentVariable('Path', ($kept -join ';'), $PathScope)
    Write-Host "PATH CHANGED: removed installer-added '$InstallDir' from the $PathScope Path environment variable."
    Remove-Item $marker -Force
} elseif (Test-Path $marker) {
    if ($scopeEntries -contains $InstallDir) {
        Write-Host "PATH unchanged: ownership marker '$owner' does not authorize $PathScope removal; pre-existing entries preserved."
    } else {
        Write-Host "PATH unchanged: '$InstallDir' not on the $PathScope Path; removing stale ownership marker."
        Remove-Item $marker -Force
    }
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
