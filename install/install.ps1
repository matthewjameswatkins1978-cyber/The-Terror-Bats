<#
.SYNOPSIS
  The Terror Bats Framework 0.2.0-rc.1 — release installer (Windows).

.DESCRIPTION
  Installs terrorbats.exe from this bundle (or a repo release build)
  into a user-local directory. No administrator rights required.
  Never modifies PATH unless -AddToUserPath is explicitly passed,
  and reports exactly what changed when it does. When the installer
  appends the directory itself, it leaves an ownership marker
  (.terrorbats-path-added) so the uninstaller only ever removes PATH
  entries this installer added — never pre-existing ones. Finishes with
  `terrorbats --version` and `terrorbats doctor` as install proof.

.PARAMETER PathScope
  Which environment scope to read/modify for the PATH entry: User
  (default, persistent) or Process (current session only; used by the
  automated regression tests so CI never touches the real user PATH).

.EXAMPLE
  .\install.ps1
  .\install.ps1 -InstallDir "$HOME\bin" -AddToUserPath
#>
param(
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA 'Programs\TerrorBats\bin'),
    [string]$SourceDir = $PSScriptRoot,
    [switch]$AddToUserPath,
    [switch]$Force,
    [ValidateSet('User', 'Process')][string]$PathScope = 'User'
)

$ErrorActionPreference = 'Stop'

$binary = Join-Path $SourceDir 'terrorbats.exe'
if (-not (Test-Path $binary)) {
    $repoBuild = Join-Path (Split-Path -Parent $SourceDir) 'target\release\terrorbats.exe'
    if (Test-Path $repoBuild) {
        $binary = $repoBuild
    } else {
        Write-Error "terrorbats.exe not found in '$SourceDir'. Run from an extracted release bundle, or build first: cargo build --release --bin terrorbats"
    }
}

if ((Test-Path $InstallDir) -and -not $Force) {
    $existing = Join-Path $InstallDir 'terrorbats.exe'
    if (Test-Path $existing) {
        Write-Error "'$existing' already exists. Re-run with -Force to replace it."
    }
}
New-Item -ItemType Directory -Force $InstallDir | Out-Null
$dest = Join-Path $InstallDir 'terrorbats.exe'
Copy-Item $binary $dest -Force
Write-Host "Installed: $dest"

$version = & $dest --version
Write-Host $version

if ($AddToUserPath) {
    $scopePath = [Environment]::GetEnvironmentVariable('Path', $PathScope)
    # Exact entry comparison: substring matching would confuse sibling
    # directories (e.g. TerrorBats-Other) or paths containing wildcards.
    $scopeEntries = @($scopePath -split ';' | Where-Object { $_ -ne '' })
    if ($scopeEntries -notcontains $InstallDir) {
        [Environment]::SetEnvironmentVariable('Path', "$scopePath;$InstallDir", $PathScope)
        # Ownership is recorded only after the append succeeds; with
        # $ErrorActionPreference='Stop' a failed write aborts before the marker.
        Set-Content -NoNewline -Encoding ascii (Join-Path $InstallDir '.terrorbats-path-added') "appended by install.ps1"
        Write-Host "PATH CHANGED: appended '$InstallDir' to the $PathScope Path environment variable."
        Write-Host "Open a new terminal for the change to take effect."
    } else {
        Write-Host "PATH unchanged: '$InstallDir' is already present; pre-existing entries are never claimed (no ownership marker written)."
    }
} else {
    Write-Host "PATH not modified (pass -AddToUserPath to change it explicitly)."
    $sessionEntries = @($env:Path -split ';' | Where-Object { $_ -ne '' })
    if ($sessionEntries -notcontains $InstallDir) {
        Write-Host "Note: '$InstallDir' is not on PATH in this session; invoke via the full path or re-open a terminal after -AddToUserPath."
    }
}

& $dest doctor
