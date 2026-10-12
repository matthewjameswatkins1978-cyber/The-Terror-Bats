"""No network, no project mutation, no MCP SDK required for these unit tests."""
from __future__ import annotations

import json
import os
import sys
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

# Ensure bridge root is in sys.path when running tests directly or via discovery
_ROOT = Path(__file__).resolve().parent.parent
if str(_ROOT) not in sys.path:
    sys.path.insert(0, str(_ROOT))

from terrorbats_engine import BridgeError, Config, Engine, from_environment


class BridgeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo = self.root / "terrorbats"
        self.repo.mkdir()
        (self.repo / "bats").mkdir()
        (self.repo / "bats" / "normal.yaml").write_text("version: terrorbat/v1\n")
        self.target = self.root / "project"
        self.target.mkdir()
        (self.target / ".git").mkdir()
        self.store = self.root / "store"
        self.config = Config(self.repo, None, self.store, {"project": self.target})
        self.engine = Engine(self.config)

    def test_readonly_by_default(self):
        with self.assertRaisesRegex(BridgeError, "Execution disabled"):
            self.engine.run("bats/normal.yaml", "project")
        with self.assertRaisesRegex(BridgeError, "Execution disabled"):
            self.engine.replay("receipt:sha256:" + "a" * 64)
        self.assertFalse(self.store.exists())

    def test_catalog_is_project_scoped(self):
        try:
            (self.repo / "bats" / "outside.yaml").symlink_to(self.root / "hostile.yaml")
        except (OSError, NotImplementedError):
            pass  # Windows may disallow unprivileged symlink creation.
        listing = self.engine.bats()
        self.assertEqual(listing["catalog"]["bats"], ["bats/normal.yaml"])
        self.assertEqual(listing["configured_targets"], ["project"])

    def test_rejects_path_traversal_and_arbitrary_paths(self):
        self.assertEqual(self.engine._bat("bats/normal.yaml"), (self.repo / "bats" / "normal.yaml").resolve())
        for bad in ("../hostile.yaml", "bats/../secret.yaml", "/etc/passwd", "bats\\normal.yaml", "packs/test.yaml"):
            with self.subTest(bad=bad), self.assertRaises(BridgeError):
                self.engine._bat(bad)

    def test_unknown_target_and_nonrepo_rejected(self):
        with self.assertRaises(BridgeError):
            self.engine._target("other")
        (self.target / ".git").rmdir()
        with self.assertRaises(BridgeError):
            self.engine._target("project")

    def test_receipt_id_must_be_canonical(self):
        for bad in ("anything", "receipt:sha256:abc", "receipt:sha256:" + "z" * 64):
            with self.subTest(bad=bad), self.assertRaises(BridgeError):
                self.engine.inspect(bad)

    def test_execution_uses_argv_and_only_returns_receipt_fields(self):
        permitted = Engine(Config(self.repo, self.root / "fake", self.store,
                                  {"project": self.target}, allow_run=True))
        record = {}
        def fake(*args, **kwargs):
            record["args"] = args
            return {"exit_code": 0, "stdout": json.dumps({"verdict":"PROVEN", "receipt_id":"receipt:sha256:"+"a"*64,
               "execution":{"secret":"DO_NOT_SHARE"}}), "stderr":"", "stdout_truncated":False}
        with patch.object(permitted, "_cmd", side_effect=fake):
            out = permitted.run("bats/normal.yaml", "project")
        self.assertEqual(out["verdict"], "PROVEN")
        self.assertNotIn("DO_NOT_SHARE", json.dumps(out))
        self.assertIn("--repo", record["args"])
        self.assertEqual(record["args"][0], "run")
        self.assertTrue(self.store.is_dir())

    def test_configuration_requires_absolute_allowlist(self):
        with patch.dict(os.environ, {"TB_PROJECTS_JSON": '{"project":"relative/path"}', "TB_REPO":str(self.repo)}):
            with self.assertRaisesRegex(BridgeError, "absolute"):
                from_environment()
        with patch.dict(os.environ, {"TB_PROJECTS_JSON": '["no"]', "TB_REPO":str(self.repo)}):
            with self.assertRaisesRegex(BridgeError, "JSON object"):
                from_environment()

    def test_timeout_is_bounded(self):
        with patch.dict(os.environ, {"TB_PROJECTS_JSON": "{}", "TB_TIMEOUT_SECONDS": "10000", "TB_REPO":str(self.repo)}):
            with self.assertRaisesRegex(BridgeError, "between 10 and 600"):
                from_environment()

    def test_replay_target_provenance_enforced(self):
        # Replaying a receipt that targets a repository outside the current allowlist
        # must fail closed and never spawn replay.
        # Furthermore, preflight uses the Rust CLI's verified `terrorbats inspect --json`,
        # which validates canonical content identity. A forged claimant in another directory
        # claiming an allowlisted target cannot fool authorization into replaying a forbidden target.
        permitted = Engine(Config(self.repo, self.root / "fake", self.store,
                                  {"project": self.target}, allow_run=True))
        receipt_id = "receipt:sha256:" + "b" * 64
        foreign_repo = self.root / "unauthorized_foreign_project"

        def fake_cmd(cmd, *args, **kwargs):
            if cmd == "inspect":
                self.assertIn("--json", args)
                # Simulated Rust CLI inspect return for the real verified receipt:
                # It reports the canonical target repository (unauthorized foreign_repo)
                return {
                    "exit_code": 0,
                    "stdout": json.dumps({
                        "version": "terrorbat/receipt/v0",
                        "receipt_id": receipt_id,
                        "target": {"repo": str(foreign_repo), "commit": "abc"}
                    }),
                    "stderr": "",
                    "stdout_truncated": False,
                    "stderr_truncated": False,
                }
            elif cmd == "replay":
                return {
                    "exit_code": 0,
                    "stdout": "replayed",
                    "stderr": "",
                    "stdout_truncated": False,
                    "stderr_truncated": False,
                }
            raise AssertionError(f"unexpected command: {cmd}")

        # Set up a raw file in store/runs where an attacker attempts to spoof an authorized target
        runs_dir = self.store / "runs" / "attacker-claimant"
        runs_dir.mkdir(parents=True)
        (runs_dir / "receipt.json").write_text(
            json.dumps({"receipt_id": receipt_id, "target": {"repo": str(self.target)}}),
            encoding="utf-8"
        )

        with patch.dict(os.environ, {"TB_ENABLE_REPLAY": "1"}):
            with patch.object(permitted, "_cmd", side_effect=fake_cmd) as mock_cmd:
                with self.assertRaisesRegex(BridgeError, "not in the currently allowlisted targets"):
                    permitted.replay(receipt_id)
                # Verify `inspect` was called, but `replay` was NEVER invoked
                called_commands = [c[0][0] for c in mock_cmd.call_args_list]
                self.assertEqual(called_commands, ["inspect"])
                self.assertNotIn("replay", called_commands)

        # Failure mode: if inspect fails (e.g. corrupt or missing receipt), replay must fail closed
        with patch.dict(os.environ, {"TB_ENABLE_REPLAY": "1"}):
            with patch.object(permitted, "_cmd", return_value={"exit_code": 1, "stdout": "", "stderr": "corrupt", "stdout_truncated": False, "stderr_truncated": False}) as mock_cmd:
                with self.assertRaisesRegex(BridgeError, "Receipt verification failed"):
                    permitted.replay(receipt_id)
                mock_cmd.assert_called_once()
                self.assertEqual(mock_cmd.call_args[0][0], "inspect")

        # Success mode: if the verified canonical receipt targets the allowlisted self.target, replay proceeds
        def fake_authorized_cmd(cmd, *args, **kwargs):
            if cmd == "inspect":
                return {
                    "exit_code": 0,
                    "stdout": json.dumps({
                        "version": "terrorbat/receipt/v0",
                        "receipt_id": receipt_id,
                        "target": {"repo": str(self.target), "commit": "def"}
                    }),
                    "stderr": "",
                    "stdout_truncated": False,
                    "stderr_truncated": False,
                }
            elif cmd == "replay":
                return {
                    "exit_code": 0,
                    "stdout": "replayed",
                    "stderr": "",
                    "stdout_truncated": False,
                    "stderr_truncated": False,
                }
            raise AssertionError(f"unexpected command: {cmd}")

        with patch.dict(os.environ, {"TB_ENABLE_REPLAY": "1"}):
            with patch.object(permitted, "_cmd", side_effect=fake_authorized_cmd) as mock_cmd:
                res = permitted.replay(receipt_id)
                self.assertTrue(res["ok"])
                called_commands = [c[0][0] for c in mock_cmd.call_args_list]
                self.assertEqual(called_commands, ["inspect", "replay"])


if __name__ == "__main__":
    unittest.main()
