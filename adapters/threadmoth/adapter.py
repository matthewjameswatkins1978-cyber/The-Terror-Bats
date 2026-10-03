"""Thin real Threadmoth CLI adapter for terrorbat-adapter/v1 (Python 3.10+)."""
import argparse
import json
from pathlib import Path
import subprocess
import sys

PROTOCOL = "terrorbat-adapter/v1"
ACTIONS = ("capabilities", "preview", "mutate", "plan", "apply_plan", "transact_preview", "transact")


def relative(root, value):
    if not isinstance(value, str) or not value:
        raise ValueError("artifact path must be a nonempty relative string")
    path = Path(value)
    if path.is_absolute() or ".." in path.parts:
        raise ValueError("artifact path must remain inside worktree")
    resolved = (root / path).resolve()
    if not resolved.is_relative_to(root):
        raise ValueError("artifact path escapes worktree")
    return resolved


def invoke(message, executable):
    payload = message["payload"]
    if not isinstance(payload, dict):
        raise ValueError("payload must be an object")
    allowed = {"request_path", "plan_path", "output_path"}
    if set(payload) - allowed:
        raise ValueError("unknown payload fields")
    action = message["action"]
    root = Path(message["context"]["worktree"]).resolve(strict=True)
    if action not in ACTIONS:
        raise ValueError("unsupported action")
    command = [executable, action.replace("_", "-")]
    if action == "transact_preview":
        command = [executable, "transact", "--preview"]
    if action in ("preview", "mutate", "plan", "transact_preview", "transact"):
        command += ["--request", str(relative(root, payload["request_path"]))]
    elif action == "apply_plan":
        command += ["--plan", str(relative(root, payload["plan_path"]))]
    forbidden = ("plan_path",) if action != "apply_plan" else ("request_path",)
    if any(key in payload for key in forbidden):
        raise ValueError("irrelevant action payload")
    if action == "capabilities" and ("request_path" in payload or "plan_path" in payload):
        raise ValueError("capabilities takes no input path")
    completed = subprocess.run(command, cwd=root, capture_output=True, timeout=25)
    try:
        document = json.loads(completed.stdout.decode("utf-8"))
    except (ValueError, UnicodeError) as error:
        raise RuntimeError("Threadmoth did not emit JSON: " + repr(completed.stdout[:300])) from error
    # Preserve the actual certificate/plan verbatim as parsed JSON; no verdict logic.
    result = {"threadmoth_exit_code": completed.returncode, "document": document}
    if "output_path" in payload:
        destination = relative(root, payload["output_path"])
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(json.dumps(result, ensure_ascii=False), encoding="utf-8")
    return completed.returncode, json.dumps(result, ensure_ascii=False), completed.stderr.decode("utf-8", "replace")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--threadmoth", default="threadmoth", help="real Threadmoth executable")
    args = parser.parse_args()
    message = json.loads(sys.stdin.readline())
    if message.get("protocol") != PROTOCOL:
        raise ValueError("unsupported protocol")
    if message.get("kind") == "describe":
        print(json.dumps({"protocol": PROTOCOL, "kind": "description", "name": "threadmoth",
                          "version": "1.0.0", "actions": {a: {"requires": ["fs.write", "process.spawn"]} for a in ACTIONS}}))
        return
    response = {"protocol": PROTOCOL, "kind": "result", "request_id": message.get("request_id", ""),
                "status": "completed", "exit_code": 3, "stdout": "", "stderr": ""}
    try:
        if set(message) != {"protocol", "kind", "request_id", "action", "payload", "context"} or message["kind"] != "execute":
            raise ValueError("invalid execute envelope")
        response["exit_code"], response["stdout"], response["stderr"] = invoke(message, args.threadmoth)
    except (ValueError, KeyError, TypeError) as error:
        response.update(status="invalid", exit_code=2, stderr=str(error))
    except (OSError, RuntimeError, subprocess.TimeoutExpired) as error:
        response.update(status="infrastructure_error", exit_code=3, stderr=str(error))
    print(json.dumps(response, ensure_ascii=False))


if __name__ == "__main__":
    main()
