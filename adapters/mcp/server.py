"""Terror Bats MCP. Local stdio by default; opt-in authenticated loopback HTTP."""
from __future__ import annotations

import argparse
import os
import sys

from terrorbats_engine import BridgeError, Engine, from_environment


def build_server(http: bool = False):
    try:
        from fastmcp import FastMCP
    except ImportError as exc:
        raise SystemExit("Install dependency: python -m pip install 'fastmcp>=2,<4'") from exc

    auth = None
    if http:
        # Never expose unauthenticated remote access to test executors.
        secret = os.environ.get("TB_MCP_AUTH_TOKEN", "")
        if len(secret) < 32:
            raise SystemExit("HTTP mode requires TB_MCP_AUTH_TOKEN, at least 32 random characters")
        from fastmcp.server.auth import StaticTokenVerifier
        auth = StaticTokenVerifier(tokens={secret: {"sub": "terrorbats-owner", "client_id": "chatgpt"}})

    mcp = FastMCP("The Terror Bats Framework", auth=auth)

    def engine() -> Engine:
        return Engine(from_environment())

    @mcp.tool
    def tb_status() -> dict:
        """Read binary version, doctor status and configured target aliases; no attacks."""
        try:
            return engine().status()
        except BridgeError as exc:
            return {"ok": False, "error": str(exc)}

    @mcp.tool
    def tb_list_bats() -> dict:
        """List permitted Bat YAML filenames and named target aliases, never arbitrary host files."""
        try:
            return engine().bats()
        except BridgeError as exc:
            return {"ok": False, "error": str(exc)}

    @mcp.tool
    def tb_run(bat: str, target_alias: str) -> dict:
        """Execute a named Bat against a PRECONFIGURED target. May execute hostile code.

        Disabled by default. Operator must explicitly enable TB_ENABLE_RUN=1
        and allowlist target aliases. Requires user's approval in the host.
        Does not accept arbitrary commands, paths or repositories.
        """
        try:
            return engine().run(bat, target_alias)
        except BridgeError as exc:
            return {"ok": False, "error": str(exc)}

    @mcp.tool
    def tb_inspect(receipt_id: str) -> dict:
        """Read a canonical existing receipt from the dedicated bridge evidence store."""
        try:
            return engine().inspect(receipt_id)
        except BridgeError as exc:
            return {"ok": False, "error": str(exc)}

    @mcp.tool
    def tb_replay(receipt_id: str) -> dict:
        """Re-execution of test receipts is deferred in MCP v0.1.0 pending M2 provenance gating.

        Unconditionally unavailable in this build.
        """
        try:
            return engine().replay(receipt_id)
        except BridgeError as exc:
            return {"ok": False, "error": str(exc)}

    return mcp


def main() -> None:
    parser = argparse.ArgumentParser(description="Terror Bats local/remote MCP bridge")
    parser.add_argument("--http", action="store_true", help="Authenticated HTTP at local loopback /mcp")
    parser.add_argument("--port", type=int, default=4319)
    args = parser.parse_args()
    if not (1 <= args.port <= 65535):
        parser.error("port must be 1-65535")
    if args.http:
        build_server(http=True).run(transport="http", host="127.0.0.1", port=args.port, path="/mcp")
    else:
        build_server(http=False).run(transport="stdio")


if __name__ == "__main__":
    main()
