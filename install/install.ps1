<#
.SYNOPSIS
  Terror Bat 0.1 First Flight — portable install helper.

.DESCRIPTION
  Copies target\release\terrorbat.exe into a user-chosen directory.
  Does NOT modify PATH unless -AddToUserPath is explicitly passed,
  and reports exactly what changed when it does.

.EXAMPLE
  .\install.ps1 -InstallDir "$HOME\bin"
  .\install.ps1 -InstallDir "$HOME\bin" -AddToUserPath
#>
param(
    [Parameter(Mandatory = $true)]
    [string]$InstallDir,

    [switch]$AddToUserPath
)

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$binary = Join-Path $repoRoot 'target\release\terrorbat.exe'

if (-not (Test-Path $binary)) {
    Write-Error "Release binary not found at $binary. Run: cargo build --release --bin terrorbat"
}

New-Item -ItemType Directory -Force $InstallDir | Out-Null
$dest = Join-Path $InstallDir 'terrorbat.exe'
Copy-Item $binary $dest -Force
Write-Host "Installed: $dest"

& $dest --version

if ($AddToUserPath) {
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if ($userPath -notlike "*$InstallDir*") {
        [Environment]::SetEnvironmentVariable('Path', "$userPath;$InstallDir", 'User')
        Write-Host "PATH CHANGED: appended '$InstallDir' to the USER Path environment variable."
        Write-Host "Open a new terminal for the change to take effect."
    } else {
        Write-Host "PATH unchanged: '$InstallDir' is already present in the USER Path."
    }
} else {
    Write-Host "PATH not modified (pass -AddToUserPath to change it explicitly)."
}
