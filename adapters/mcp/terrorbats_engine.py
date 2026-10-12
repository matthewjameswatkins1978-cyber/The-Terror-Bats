"""Bounded, local Terror Bats CLI adapter shared by MCP and ordinary callers.

This module intentionally has no model or MCP dependency. It never passes
user-selected strings through a shell, and never accepts arbitrary target paths.
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import tempfile
from dataclasses import dataclass
from typing import Any

MAX_READ = 65_536
MAX_BATS = 300
RECEIPT = re.compile(r"^receipt:sha256:[0-9a-f]{64}$")
ALIAS = re.compile(r"^[a-zA-Z0-9][a-zA-Z0-9_-]{0,63}$")


class BridgeError(Exception):
    """A safe, user-facing, non-executable bridge failure."""


@dataclass(frozen=True)
class Config:
    repo: Path
    binary: Path | None
    store: Path
    targets: dict[str, Path]
    allow_run: bool = False
    timeout_seconds: int = 120


def _real(value: str | Path) -> Path:
    return Path(value).expanduser().resolve(strict=False)


def _within(path: Path, root: Path) -> bool:
    return path == root or root in path.parents


def from_environment() -> Config:
    repo = _real(os.environ.get("TB_REPO", Path.cwd()))
    binary_str = os.environ.get("TB_BIN") or shutil.which("terrorbats")
    if not binary_str:
        path = repo / "target" / "release" / ("terrorbats.exe" if os.name == "nt" else "terrorbats")
        binary_str = str(path) if path.is_file() else None
    raw = os.environ.get("TB_PROJECTS_JSON", "{}")
    try:
        mapping = json.loads(raw)
    except json.JSONDecodeError as exc:
        raise BridgeError("TB_PROJECTS_JSON must be a JSON object of target aliases to directories") from exc
    if not isinstance(mapping, dict):
        raise BridgeError("TB_PROJECTS_JSON must be a JSON object")
    targets: dict[str, Path] = {}
    for alias, loc in mapping.items():
        if not isinstance(alias, str) or not ALIAS.fullmatch(alias) or not isinstance(loc, str) or not loc:
            raise BridgeError("Invalid project alias or directory in TB_PROJECTS_JSON")
        if not Path(loc).is_absolute():
            raise BridgeError("Project paths must be absolute")
        targets[alias] = _real(loc)
    timeout = int(os.environ.get("TB_TIMEOUT_SECONDS", "120"))
    if timeout < 10 or timeout > 600:
        raise BridgeError("TB_TIMEOUT_SECONDS must be between 10 and 600")
    return Config(
        repo=repo,
        binary=_real(binary_str) if binary_str else None,
        store=_real(os.environ.get("TB_STORE", repo / ".terrorbats-mcp-evidence")),
        targets=targets,
        allow_run=os.environ.get("TB_ENABLE_RUN", "0") == "1",
        timeout_seconds=timeout,
    )


class Engine:
    def __init__(self, config: Config):
        self.config = config

    def _bin(self) -> str:
        path = self.config.binary
        if path is None or not path.is_file():
            raise BridgeError("Terror Bats executable is missing; set TB_BIN to the installed terrorbats executable")
        return str(path)

    def _cmd(self, *args: str, timeout: int | None = None) -> dict[str, Any]:
        argv = [self._bin(), *args]
        # Output is spooled to disk so a hostile child cannot exhaust the bridge RAM.
        with tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
            kw: dict[str, Any] = {"stdout": stdout, "stderr": stderr, "stdin": subprocess.DEVNULL}
            if os.name == "nt":
                kw["creationflags"] = subprocess.CREATE_NEW_PROCESS_GROUP
            else:
                kw["start_new_session"] = True
            try:
                proc = subprocess.Popen(argv, **kw)
                try:
                    code = proc.wait(timeout=min(timeout or self.config.timeout_seconds, 600))
                except subprocess.TimeoutExpired as exc:
                    if os.name == "nt":
                        subprocess.run(["taskkill", "/F", "/T", "/PID", str(proc.pid)],
                                       capture_output=True, timeout=10, check=False)
                    else:
                        try:
                            os.killpg(proc.pid, signal.SIGKILL)
                        except ProcessLookupError:
                            pass
                    proc.wait(timeout=10)
                    raise BridgeError("Terror Bats exceeded its execution timeout; process group termination attempted") from exc
            except OSError as exc:
                raise BridgeError(f"Could not start Terror Bats: {exc.strerror or type(exc).__name__}") from exc
            stdout.seek(0, 2)
            stdout_size = stdout.tell()
            stdout.seek(0)
            out = stdout.read(MAX_READ).decode("utf-8", errors="replace")
            stderr.seek(0, 2)
            stderr_size = stderr.tell()
            stderr.seek(0)
            err = stderr.read(4096).decode("utf-8", errors="replace")
        return {"exit_code": code, "stdout": out, "stderr": err,
                "stdout_truncated": stdout_size > MAX_READ,
                "stderr_truncated": stderr_size > 4096}

    def status(self) -> dict[str, Any]:
        version = self._cmd("--version", timeout=30)
        if version["exit_code"] != 0:
            raise BridgeError("terrorbats --version did not succeed")
        doctor = self._cmd("doctor", "--json", timeout=90)
        response: dict[str, Any] = {"version": version["stdout"].strip(),
                                     "doctor_exit_code": doctor["exit_code"],
                                     "run_enabled": self.config.allow_run,
                                     "configured_targets": sorted(self.config.targets)}
        try:
            response["doctor"] = json.loads(doctor["stdout"])
        except json.JSONDecodeError:
            response["doctor_error"] = "Doctor did not return valid JSON"
        return response

    def bats(self) -> dict[str, Any]:
        root = self.config.repo
        items: dict[str, list[str]] = {}
        for dirname in ("bats", "packs"):
            base = root / dirname
            if not base.is_dir():
                items[dirname] = []
                continue
            names = []
            for candidate in sorted(base.rglob("*.yaml")):
                resolved = candidate.resolve()
                if candidate.is_file() and _within(resolved, base.resolve()):
                    names.append(candidate.relative_to(root).as_posix())
                if len(names) >= MAX_BATS:
                    break
            items[dirname] = names
        return {"catalog": items, "configured_targets": sorted(self.config.targets),
                "run_enabled": self.config.allow_run}

    def _bat(self, relative: str) -> Path:
        if not isinstance(relative, str) or len(relative) > 256 or "\\" in relative:
            raise BridgeError("Bat must be a relative .yaml name from the catalog")
        rel = Path(relative)
        if rel.is_absolute() or not relative.startswith("bats/") or rel.suffix != ".yaml":
            raise BridgeError("Only cataloged bats/*.yaml may be executed")
        root = (self.config.repo / "bats").resolve()
        candidate = (self.config.repo / rel).resolve()
        if not _within(candidate, root) or not candidate.is_file() or candidate.is_symlink():
            raise BridgeError("Bat path is missing or escapes the allowed bats directory")
        return candidate

    def _target(self, alias: str) -> Path:
        if alias not in self.config.targets:
            raise BridgeError("Unknown target alias. Use tb_list_bats to see configured targets")
        target = self.config.targets[alias]
        if not target.is_dir():
            raise BridgeError("Configured target directory is missing")
        if not (target / ".git").exists():
            raise BridgeError("Configured target must be a Git worktree/repository")
        return target

    def _write_allowed(self) -> None:
        if not self.config.allow_run:
            raise BridgeError("Execution disabled. Operator must explicitly set TB_ENABLE_RUN=1 and configure named targets")
        if not self.config.targets:
            raise BridgeError("No allowed targets. Configure TB_PROJECTS_JSON before enabling execution")

    def run(self, bat: str, target_alias: str) -> dict[str, Any]:
        self._write_allowed()
        path = self._bat(bat)
        target = self._target(target_alias)
        self.config.store.mkdir(parents=True, exist_ok=True)
        result = self._cmd("run", str(path), "--repo", str(target),
                           "--store", str(self.config.store), "--json")
        if result["exit_code"] != 0:
            return {"ok": False, "exit_code": result["exit_code"],
                    "error": "Run failed. Inspect details locally; child stderr may contain secrets",
                    "stderr_truncated": result["stderr_truncated"]}
        if result["stdout_truncated"]:
            raise BridgeError("Run JSON exceeded bridge response cap; inspect evidence locally")
        try:
            payload = json.loads(result["stdout"])
        except json.JSONDecodeError as exc:
            raise BridgeError("Run returned invalid JSON") from exc
        # Deliberately omit raw captured stdout/stderr/evidence from MCP replies.
        return {"ok": True, "verdict": payload.get("verdict"),
                "receipt_id": payload.get("receipt_id"),
                "target_alias": target_alias, "bat": bat,
                "notice": "Child output and evidence may contain secrets; inspect locally before sharing"}

    def inspect(self, receipt_id: str) -> dict[str, Any]:
        if not RECEIPT.fullmatch(receipt_id):
            raise BridgeError("Expected canonical receipt:sha256:<64 hex> identifier")
        result = self._cmd("inspect", receipt_id, "--store", str(self.config.store), timeout=40)
        return {"ok": result["exit_code"] == 0, "exit_code": result["exit_code"],
                "inspection": result["stdout"], "truncated": result["stdout_truncated"],
                "notice": "Inspection may disclose paths or captured target data"}

    def replay(self, receipt_id: str) -> dict[str, Any]:
        self._write_allowed()
        if os.environ.get("TB_ENABLE_REPLAY", "0") != "1":
            raise BridgeError("Replay is disabled separately; the operator must set TB_ENABLE_REPLAY=1")
        if not RECEIPT.fullmatch(receipt_id):
            raise BridgeError("Expected canonical receipt:sha256:<64 hex> identifier")
        result = self._cmd("replay", receipt_id, "--store", str(self.config.store))
        return {"ok": result["exit_code"] == 0, "exit_code": result["exit_code"],
                "summary": result["stdout"], "truncated": result["stdout_truncated"]}
