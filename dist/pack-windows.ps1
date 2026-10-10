<#
.SYNOPSIS
  Assemble the Windows RC1 bundle locally or in CI (assembly only;
  gates and the release build run separately in the release workflow).

.DESCRIPTION
  Stages terrorbats.exe, docs, licences, installers, generated
  completions, brand assets (when present) and BUILDINFO.json, then
  writes terrorbats-<version>-windows-x86_64.zip plus a .sha256 file.
#>
param(
    [string]$RepoRoot = (Split-Path -Parent $PSScriptRoot),
    [string]$Binary = '',
    [string]$OutDir = (Join-Path $RepoRoot 'dist'),
    [string]$Version = '',
    [string]$BuildWorkflow = 'local'
)

$ErrorActionPreference = 'Stop'

if ($Binary -eq '') { $Binary = Join-Path $RepoRoot 'target\release\terrorbats.exe' }
if (-not (Test-Path $Binary)) { Write-Error "Binary not found: $Binary (build first: cargo build --release --bin terrorbats)" }
if ($Version -eq '') {
    $Version = (& $Binary --version) -replace '^terrorbats ', ''
    if (-not $Version) { Write-Error "Could not determine version from $Binary" }
}
$commit = (git -C $RepoRoot rev-parse HEAD).Trim()
$rustc = (rustc --version)
$artifact = "terrorbats-$Version-windows-x86_64"

$stage = Join-Path ([System.IO.Path]::GetTempPath()) "terrorbats-dist-$Version-win"
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Force $stage, "$stage\completions" | Out-Null

Copy-Item $Binary (Join-Path $stage 'terrorbats.exe')
foreach ($doc in @('README.md', 'CHANGELOG.md', 'LICENSE-MIT', 'LICENSE-APACHE')) {
    Copy-Item (Join-Path $RepoRoot $doc) $stage
}
Copy-Item (Join-Path $RepoRoot 'docs\MANUAL.md') (Join-Path $stage 'MANUAL.md')
Copy-Item (Join-Path $RepoRoot 'install\install.ps1') $stage
Copy-Item (Join-Path $RepoRoot 'install\uninstall.ps1') $stage

$bin = Join-Path $stage 'terrorbats.exe'
& $bin completions powershell > (Join-Path $stage 'completions\terrorbats.ps1')
& $bin completions bash > (Join-Path $stage 'completions\terrorbats.bash')
& $bin completions zsh > (Join-Path $stage 'completions\terrorbats.zsh')
& $bin completions fish > (Join-Path $stage 'completions\terrorbats.fish')

$brand = Join-Path $RepoRoot 'assets\brand'
if (Test-Path $brand) {
    Copy-Item $brand (Join-Path $stage 'assets\brand') -Recurse
} else {
    Write-Host "NOTE: assets/brand/ not present; bundle ships without brand assets."
}

$buildinfo = [ordered]@{
    product = "The Terror Bats Framework"
    version = $Version
    git_commit = $commit
    target = "x86_64-pc-windows-msvc"
    rust_version = $rustc
    build_profile = "release"
    build_workflow = $BuildWorkflow
    artifact_name = "$artifact.zip"
} | ConvertTo-Json
Set-Content -NoNewline -Encoding utf8 (Join-Path $stage 'BUILDINFO.json') $buildinfo

New-Item -ItemType Directory -Force $OutDir | Out-Null
$zip = Join-Path $OutDir "$artifact.zip"
if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip
$hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
Set-Content -NoNewline -Encoding ascii "$zip.sha256" "$hash  $artifact.zip"
Remove-Item $stage -Recurse -Force

Write-Host "Bundle: $zip"
Write-Host "SHA256: $hash"
