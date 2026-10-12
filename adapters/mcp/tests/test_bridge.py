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
        # Even when run and replay are both enabled, replaying a receipt that targets
        # a repository outside the current allowlist must fail closed and never spawn replay.
        permitted = Engine(Config(self.repo, self.root / "fake", self.store,
                                  {"project": self.target}, allow_run=True))
        receipt_id = "receipt:sha256:" + "b" * 64
        # Set up a receipt in store/runs that targets an unallowlisted foreign repository
        foreign_repo = self.root / "unauthorized_foreign_project"
        foreign_repo.mkdir()
        run_dir = self.store / "runs" / "test-run-123"
        run_dir.mkdir(parents=True)
        receipt_data = {
            "receipt_id": receipt_id,
            "target": {"repo": str(foreign_repo), "commit": "abc"},
        }
        (run_dir / "receipt.json").write_text(json.dumps(receipt_data), encoding="utf-8")

        with patch.dict(os.environ, {"TB_ENABLE_REPLAY": "1"}):
            with patch.object(permitted, "_cmd") as mock_cmd:
                with self.assertRaisesRegex(BridgeError, "not in the currently allowlisted targets"):
                    permitted.replay(receipt_id)
                mock_cmd.assert_not_called()

        # Conversely, if receipt targets the allowlisted self.target, it is authorized to spawn replay
        authorized_receipt_id = "receipt:sha256:" + "c" * 64
        auth_run_dir = self.store / "runs" / "test-run-auth"
        auth_run_dir.mkdir(parents=True)
        auth_receipt_data = {
            "receipt_id": authorized_receipt_id,
            "target": {"repo": str(self.target), "commit": "def"},
        }
        (auth_run_dir / "receipt.json").write_text(json.dumps(auth_receipt_data), encoding="utf-8")

        with patch.dict(os.environ, {"TB_ENABLE_REPLAY": "1"}):
            with patch.object(permitted, "_cmd", return_value={"exit_code": 0, "stdout": "replayed", "stdout_truncated": False}) as mock_cmd:
                res = permitted.replay(authorized_receipt_id)
                self.assertTrue(res["ok"])
                mock_cmd.assert_called_once()
                self.assertEqual(mock_cmd.call_args[0][0], "replay")


if __name__ == "__main__":
    unittest.main()
