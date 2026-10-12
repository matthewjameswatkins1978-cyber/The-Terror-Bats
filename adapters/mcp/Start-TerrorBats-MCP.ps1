<#
Start the private Terror Bats bridge on Windows (PowerShell 7 recommended).
Defaults: local loopback HTTP, read-only, bearer authentication, no tunnels.
-ToRun and -TargetPath must both be given to enable any new attacks.
-EnableReplay must additionally be given to enable replay of existing receipts.
#>
[CmdletBinding()]
param(
    [string]$Repo = 'D:\Projects\The-Terror-Bats',
    [string]$TargetPath = '',
    [switch]$AllowRun,
    [switch]$EnableReplay,
    [switch]$ShowToken,
    [int]$Port = 4319
)
$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
if (-not (Test-Path (Join-Path $Repo 'bats'))) {
    throw "Terror Bats repo not found at $Repo. Supply -Repo <repo-path>."
}
if ($AllowRun -and -not $TargetPath) {
    throw 'Enabling execution requires -TargetPath to allowlist a single target repository.'
}
if ($EnableReplay -and -not $AllowRun) {
    throw 'Replay also requires -AllowRun and a named target.'
}
$bin = (Get-Command 'terrorbats.exe' -ErrorAction SilentlyContinue).Source
if (-not $bin) {
    foreach ($candidate in @(
        (Join-Path $Repo 'target\release\terrorbats.exe'),
        (Join-Path $env:LOCALAPPDATA 'Programs\TerrorBats\bin\terrorbats.exe')
    )) {
        if (Test-Path $candidate) { $bin = $candidate; break }
    }
}
if (-not $bin) {
    throw 'terrorbats.exe not found. Install the v1.0.0-rc.1 Windows bundle or build the release binary.'
}
$env:TB_REPO = (Resolve-Path $Repo).Path
$env:TB_BIN = (Resolve-Path $bin).Path
$env:TB_STORE = Join-Path $env:LOCALAPPDATA 'TerrorBats\McpEvidence'
$env:TB_ENABLE_RUN = if ($AllowRun) { '1' } else { '0' }
$env:TB_ENABLE_REPLAY = if ($EnableReplay) { '1' } else { '0' }
$env:TB_PROJECTS_JSON = '{}'
if ($TargetPath) {
    if (-not (Test-Path (Join-Path $TargetPath '.git'))) {
        throw 'TargetPath must be an existing Git repository/worktree with a .git entry.'
    }
    $target = (Resolve-Path $TargetPath).Path
    $alias = (Split-Path $target -Leaf) -replace '[^a-zA-Z0-9_-]', '-'
    if ($alias.Length -gt 64) { $alias = $alias.Substring(0, 64) }
    if ($alias -notmatch '^[a-zA-Z0-9]') { $alias = 'project-' + $alias }
    $targetMap = @{}
    $targetMap[$alias] = $target
    $env:TB_PROJECTS_JSON = $targetMap | ConvertTo-Json -Compress
    Write-Host "Configured target alias: $alias" -ForegroundColor Cyan
}
# The user-scoped token is encrypted by DPAPI; never commit, log or copy it
# into a public plugin manifest. Pass it to ChatGPT only through auth setup.
$tokenDir = Join-Path $env:APPDATA 'TerrorBats\MCP'
New-Item -ItemType Directory -Path $tokenDir -Force | Out-Null
$tokenFile = Join-Path $tokenDir 'auth.dpapi'
if (Test-Path $tokenFile) {
    $secure = ConvertTo-SecureString ((Get-Content $tokenFile -Raw).Trim())
    $env:TB_MCP_AUTH_TOKEN = [System.Net.NetworkCredential]::new('', $secure).Password
} else {
    $bytes = [System.Security.Cryptography.RandomNumberGenerator]::GetBytes(32)
    $token = [Convert]::ToBase64String($bytes).Replace('+','-').Replace('/','_').TrimEnd('=')
    $secure = ConvertTo-SecureString -String $token -AsPlainText -Force
    $secure | ConvertFrom-SecureString | Set-Content -Encoding ascii $tokenFile
    $env:TB_MCP_AUTH_TOKEN = $token
}
if ($ShowToken) {
    Write-Warning 'Keep this connection token private. Do not paste it into chat, GitHub or logs.'
    Write-Host $env:TB_MCP_AUTH_TOKEN
}
$venv = Join-Path $root '.venv'
if (-not (Test-Path (Join-Path $venv 'Scripts\python.exe'))) {
    # Prefer a working python interpreter on PATH first.
    # Fall back to the py launcher only if verified functional (handles broken py shims/wrappers).
    $pyHost = $null
    if (Get-Command 'python' -ErrorAction SilentlyContinue) {
        $ver = & python -c 'import sys; print(f"{sys.version_info[0]}.{sys.version_info[1]}")' 2>$null
        if ($LASTEXITCODE -eq 0 -and $ver -match '^3\.(1[0-9]|[2-9][0-9])') {
            $pyHost = 'python'
        }
    }
    if (-not $pyHost -and (Get-Command 'py' -ErrorAction SilentlyContinue)) {
        & py -3.13 -c 'import sys' 2>$null
        if ($LASTEXITCODE -eq 0) {
            $pyHost = 'py -3.13'
        } else {
            & py -3 -c 'import sys' 2>$null
            if ($LASTEXITCODE -eq 0) { $pyHost = 'py -3' }
        }
    }
    if (-not $pyHost) {
        throw 'Compatible Python (>=3.10) interpreter not found. Ensure python is on PATH.'
    }
    if ($pyHost -eq 'python') {
        & python -m venv $venv
    } elseif ($pyHost -eq 'py -3.13') {
        & py -3.13 -m venv $venv
    } else {
        & py -3 -m venv $venv
    }
    if ($LASTEXITCODE -ne 0) { throw 'Unable to create Python environment.' }
}
$python = Join-Path $venv 'Scripts\python.exe'
& $python -c 'import fastmcp' 2>$null
if ($LASTEXITCODE -ne 0) {
    & $python -m pip install 'fastmcp>=2,<4'
    if ($LASTEXITCODE -ne 0) { throw 'Unable to install FastMCP dependency.' }
}
Write-Host "Local bridge: http://127.0.0.1:$Port/mcp" -ForegroundColor Green
Write-Host 'No public connection is started. Expose only through an authenticated HTTPS tunnel.'
Write-Host 'Attack execution:' $env:TB_ENABLE_RUN ' / Replay:' $env:TB_ENABLE_REPLAY
& $python (Join-Path $root 'server.py') --http --port $Port
