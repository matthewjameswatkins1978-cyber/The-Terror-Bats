"""Real adapter and checker sensitivity controls (no synthetic defect is a finding)."""
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
ADAPTER = ROOT / "adapters/threadmoth/adapter.py"
FIXTURES = ROOT / "adapters/threadmoth/fixtures.py"
TERRORBAT = ROOT / ("target/debug/terrorbats.exe" if os.name == "nt" else "target/debug/terrorbat")
THREADMOTH = shutil.which("threadmoth")


class RealAdapter(unittest.TestCase):
    def setUp(self):
        if not THREADMOTH:
            self.skipTest("real Threadmoth executable unavailable")
        self.temp = tempfile.TemporaryDirectory()
        self.work = Path(self.temp.name)
        self.addCleanup(self.temp.cleanup)

    def adapter(self, action, payload):
        request = {"protocol": "terrorbat-adapter/v1", "kind": "execute", "request_id": "test-request",
                   "action": action, "payload": payload,
                   "context": {"worktree": str(self.work), "target_commit": "fixture", "execution_id": "test",
                               "phase": "run", "step_index": 0}}
        p = subprocess.run([sys.executable, str(ADAPTER), "--threadmoth", THREADMOTH],
                           input=json.dumps(request), text=True, capture_output=True, check=True)
        self.assertEqual(len(p.stdout.splitlines()), 1)
        return json.loads(p.stdout)

    def fixture(self, action, case="certificate-byte-truth"):
        p = subprocess.run([sys.executable, str(FIXTURES), action, case], cwd=self.work,
                           capture_output=True, text=True)
        return p, json.loads(p.stdout) if p.stdout.strip() else None

    def test_description_uses_real_operations(self):
        p = subprocess.run([sys.executable, str(ADAPTER)], input=json.dumps(
            {"protocol": "terrorbat-adapter/v1", "kind": "describe"}), text=True, capture_output=True, check=True)
        self.assertEqual(json.loads(p.stdout)["name"], "threadmoth")
        self.assertIn("apply_plan", json.loads(p.stdout)["actions"])

    def test_unknown_payload_is_invalid_with_integer_exit(self):
        result = self.adapter("mutate", {"request_path": "missing.json", "unknown": 1})
        self.assertEqual(result["status"], "invalid")
        self.assertEqual(result["exit_code"], 2)
        self.assertFalse(list(self.work.iterdir()))

    def test_artifact_escape_is_invalid(self):
        result = self.adapter("mutate", {"request_path": "../outside.json"})
        self.assertEqual(result["status"], "invalid")
        self.assertEqual(result["exit_code"], 2)

    def test_real_refusal_preserves_certificate_and_exit_two(self):
        self.fixture("prepare", "ambiguous-exact-match")
        result = self.adapter("mutate", {"request_path": ".hostile/request.json", "output_path": ".hostile/result.json"})
        self.assertEqual(result["status"], "completed")
        self.assertEqual(result["exit_code"], 2)
        self.assertEqual(json.loads(result["stdout"])["document"]["reason_code"], "TARGET_AMBIGUOUS")
        _, facts = self.fixture("check", "ambiguous-exact-match")
        self.assertIs(facts["violation"], False)

    def test_independent_checker_detects_certificate_lie(self):
        self.fixture("prepare")
        result = self.adapter("mutate", {"request_path": ".hostile/request.json", "output_path": ".hostile/result.json"})
        self.assertEqual(result["exit_code"], 0)
        _, healthy = self.fixture("check")
        self.assertIs(healthy["violation"], False)
        artifact = self.work / ".hostile/result.json"
        cert = json.loads(artifact.read_text())
        cert["document"]["post_hash"] = "0" * 64
        artifact.write_text(json.dumps(cert))
        _, lied = self.fixture("check")
        self.assertIs(lied["violation"], True)
        artifact.unlink()
        _, missing = self.fixture("check")
        self.assertNotIn("violation", missing)
        self.assertIs(missing["valid"], False)

    def test_old_candidate_with_current_hash_is_rejected(self):
        case = "candidate-guard-staleness"
        self.fixture("prepare", case)
        self.adapter("preview", {"request_path": ".hostile/request.json", "output_path": ".hostile/first.json"})
        p, _ = self.fixture("intervene", case)
        self.assertEqual(p.returncode, 0)
        first = self.adapter("mutate", {"request_path": ".hostile/request.json", "output_path": ".hostile/result.json"})
        second = self.adapter("mutate", {"request_path": ".hostile/request2.json", "output_path": ".hostile/result2.json"})
        self.assertEqual(json.loads(first["stdout"])["document"]["reason_code"], "STALE_IDENTITY")
        self.assertEqual(json.loads(second["stdout"])["document"]["reason_code"], "CANDIDATE_SELECTION_INVALID")
        _, facts = self.fixture("check", case)
        self.assertIs(facts["violation"], False)

    def test_terrorbat_oracle_direction_with_synthetic_controls(self):
        if not TERRORBAT.exists():
            self.skipTest("build terrorbat before running CLI controls")
        target = self.work / "target"
        target.mkdir()
        subprocess.run(["git", "init", str(target)], check=True, capture_output=True)
        subprocess.run(["git", "-C", str(target), "config", "user.name", "Checker Control"], check=True)
        subprocess.run(["git", "-C", str(target), "config", "user.email", "control@localhost"], check=True)
        (target / "root.txt").write_text("disposable")
        subprocess.run(["git", "-C", str(target), "add", "."], check=True)
        subprocess.run(["git", "-C", str(target), "commit", "-m", "Control fixture"], check=True, capture_output=True)
        bindings = self.work / "bindings.json"
        bindings.write_text(json.dumps({"version": "terrorbat-adapters/v1", "adapters": {
            "threadmoth": {"program": sys.executable, "args": [str(ADAPTER), "--threadmoth", THREADMOTH]}}}))
        for mode, expected in (("healthy", "NOT OBSERVED"), ("lie", "PROVEN"), ("missing", "INCONCLUSIVE")):
            run = [{"adapter": "threadmoth", "action": "mutate", "request_path": ".hostile/request.json",
                    "output_path": ".hostile/result.json"}]
            if mode == "lie":
                code = ("import json,pathlib;p=pathlib.Path('.hostile/result.json');"
                        "d=json.loads(p.read_text());d['document']['post_hash']='0'*64;p.write_text(json.dumps(d))")
                run.append({"adapter": "command", "action": "run", "program": sys.executable, "args": ["-c", code]})
            elif mode == "missing":
                run.append({"adapter": "command", "action": "run", "program": sys.executable,
                            "args": ["-c", "import pathlib;pathlib.Path('.hostile/result.json').unlink()"]})
            run.append({"adapter": "command", "action": "run", "program": sys.executable,
                        "args": [str(FIXTURES), "check", "certificate-byte-truth"]})
            bat = {"version": "terrorbat/v1", "id": "synthetic-checker-control-" + mode,
                   "claim": {"text": "Synthetic checker sensitivity control; never a Threadmoth defect finding."},
                   "requires": ["fs.write", "process.spawn", "git.inspect"],
                   "attack": {"setup": [{"adapter": "command", "action": "run", "program": sys.executable,
                                         "args": [str(FIXTURES), "prepare", "certificate-byte-truth"]}], "run": run},
                   "oracle": {"type": "json_value_equals", "step": "run:" + str(len(run) - 1),
                              "pointer": "/violation", "value": True},
                   "evidence": {"capture": ["stdout", "stderr", "git_diff"]},
                   "timeout": {"setup": "30s", "run": "60s", "total": "90s"}}
            spec = self.work / ("control-" + mode + ".json")
            spec.write_text(json.dumps(bat))
            result = subprocess.run([str(TERRORBAT), "run", str(spec), "--repo", str(target),
                                     "--adapters", str(bindings), "--store", str(self.work / "store"), "--json"],
                                    capture_output=True, text=True)
            receipt = json.loads(result.stdout)
            self.assertEqual(receipt["verdict"], expected, result.stderr + result.stdout)
        self.assertEqual(subprocess.run(["git", "-C", str(target), "status", "--porcelain"],
                                       capture_output=True, text=True, check=True).stdout, "")

    def test_missing_executable_is_infrastructure_with_integer_exit(self):
        module_spec = importlib.util.spec_from_file_location("thin_adapter", ADAPTER)
        module = importlib.util.module_from_spec(module_spec)
        module_spec.loader.exec_module(module)
        with self.assertRaises(OSError):
            module.invoke({"action": "capabilities", "payload": {}, "context": {"worktree": str(self.work)}},
                          str(self.work / "missing-executable"))


if __name__ == "__main__":
    unittest.main()
