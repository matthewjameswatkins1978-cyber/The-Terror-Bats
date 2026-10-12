# Terror Bats MCP Bridge · 0.1.0

A **working integration build**, not a second testing engine. The existing Terror Bats CLI remains authoritative for execution, evidence, receipts and replay. This Python MCP wrapper provides five structured tools to model clients: `tb_status`, `tb_list_bats`, `tb_run`, `tb_inspect` and `tb_replay`.

## What this build does and doesn't do

- **Works locally:** launch a standard MCP stdio server for agent clients, or a bearer-protected HTTP server bound to `127.0.0.1:4319`.
- **Can work with ChatGPT:** once the HTTP endpoint is exposed by a *secure HTTPS tunnel* and registered as a **custom MCP server** in ChatGPT, the tools can be called there. Merely installing this ZIP does not give ChatGPT remote access. You must verify live tool discovery and one real call before claiming the connection works.
- **Default is read-only:** `tb_run` fails closed unless `TB_ENABLE_RUN=1` and an explicit named-target allowlist is configured. `tb_replay` is unconditionally disabled in MCP v0.1.0 pending M2 cryptographic target provenance gating; the authoritative Terror Bats CLI remains available directly for receipt replay.
- The bridge never accepts arbitrary shell commands, GitHub refs, or target filesystem paths from an assistant. It uses subprocess argument arrays (no shell), bounded waits and output, refuses Bat-path escape, and returns only verdict/receipt metadata for a new run, not captured child output.
- **Not a sandbox:** the tests you explicitly author may run programs with the operating system user's privileges; worktrees are not containment. Captured child output/evidence may contain secrets. Do not expose the local port directly to the internet or enable execution on projects you cannot afford to alter.

## Windows quick start

Requirements: Python 3.13, the official Terror Bats `v1.0.0-rc.1` Windows binary installed or present at `D:\Projects\The-Terror-Bats\target\release\terrorbats.exe`, and the checked-out repository (for the Bat catalog).

1. Extract this ZIP to a folder you control and open PowerShell 7 there.
2. Run **read-only**, with no target permission:

   ```powershell
   .\Start-TerrorBats-MCP.ps1
   ```

   This creates an isolated Python virtual environment, installs the **FastMCP** library (if absent), creates a random bearer token securely stored for your Windows account using DPAPI, and serves at `http://127.0.0.1:4319/mcp`. It does **not** start a tunnel.

3. To allow a single named local Git project for attacks, stop and restart deliberately:

   ```powershell
   .\Start-TerrorBats-MCP.ps1 -AllowRun -TargetPath 'D:\Projects\Threadmoth'
   ```

   To allow replay as well, add `-EnableReplay`. Avoid sharing the token in chat. A `-ShowToken` switch exists only for entering it into a private connection credential field.

4. For local agent clients that support stdio (Pi, compatible IDE agents), instead install the dependency and launch:

   ```powershell
   py -3.13 -m pip install 'fastmcp>=2,<4'
   $env:TB_REPO = 'D:\Projects\The-Terror-Bats'
   $env:TB_BIN  = 'D:\Projects\The-Terror-Bats\target\release\terrorbats.exe'
   py -3.13 server.py
   ```

   A stdio MCP tool can be invoked by your agent after configuring that agent's MCP server command. Stdio inherits Windows account permissions; start with read-only mode.

## Connect this to Lucy in ChatGPT

ChatGPT cannot call `http://127.0.0.1:4319` from its cloud session. You need a **stable HTTPS URL** routed to this local port, for example a dedicated Cloudflare Tunnel hostname. It must not expose an unauthenticated instance or other local services.

Use **ChatGPT → Plugins → Add custom MCP server**, supply the HTTPS `/mcp` URL, and configure the server authentication as a bearer/API-key credential where available. The bridge refuses HTTP startup without a random token of at least 32 characters. **API-key registration may not be supported on every ChatGPT installation path**; if the UI cannot accept the required credential, do **not** disable auth to make it work. Use an OAuth-capable endpoint or an approved secure proxy instead. Register the server, verify tools appear, then call `tb_status` and `tb_list_bats` before enabling any writes. OpenAI's current plugin packaging guidance requires registering the MCP connection separately and then mapping its registered app ID into the plugin package.

When live registration works, Lucy can list bats, run allowed named specimens, inspect receipts, replay by explicit permission, and compare findings in chat. **Until that connection is established and tested, this ZIP is only a local agent-usable build, not a callable tool in the current conversation.**

## Environment and limits

| Setting | Meaning |
|---|---|
| `TB_REPO` | The checked-out Terror Bats repository with `bats/` and `packs/` |
| `TB_BIN` | Absolute installed binary path; otherwise resolved from PATH or `target/release` |
| `TB_STORE` | Bridge-owned evidence store; keep separate from ordinary repos |
| `TB_PROJECTS_JSON` | JSON alias→absolute Git worktree mapping, e.g. `{"threadmoth":"D:\\Projects\\Threadmoth"}` |
| `TB_ENABLE_RUN` | `1` enables named-target attacks; default `0` |
| `TB_ENABLE_REPLAY` | `1` additionally enables replay; default `0` |
| `TB_TIMEOUT_SECONDS` | 10–600, default 120 |
| `TB_MCP_AUTH_TOKEN` | Random secret for authenticated HTTP, never put in source control |

## Checks already run on this package

`python -m compileall -q .` and `python -m unittest discover -s tests -v`: 8 core tests pass for fail-closed permissions, path traversal, named targets, receipt validation, output redaction, bounded timeout and configuration. HTTP wire-up and actual execution against the user's installed Windows binary require a connected machine and **are not claimed tested here**.

## Deferred enhancements

- Signed authorization grants / per-target human approval in the bridge, rather than only host tool permissions and environment allowlisting.
- Native checks that stored receipts cannot reference a different target before replay.
- Secure OAuth-based identity for remote deployments and a complete cross-platform MCP integration test.
- More compact structured `inspect --json` when the upstream CLI supports it.
