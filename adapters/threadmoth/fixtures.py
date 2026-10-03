"""Disposable fixtures and independent byte/certificate measurements; not a production adapter."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

CASES = ("stale-preview", "stale-plan", "tampered-plan", "path-prefix-escape",
         "junction-symlink-escape", "ambiguous-exact-match", "candidate-guard-staleness",
         "hard-budget-overrun", "crlf-preservation", "unicode-bom", "transaction-rollback",
         "certificate-byte-truth", "replayed-identical-request", "unknown-fields-strictness",
         "malformed-structured-no-fallback")


def write_json(path, value):
    Path(path).write_text(json.dumps(value, ensure_ascii=False), encoding="utf-8")


def request(path="target.txt"):
    return {"version": "1.3.1", "request_id": "hostile-fixture", "file_path": path,
            "cardinality": {"type": "exactly_one"},
            "budget": {"allowed_path_prefixes": [], "max_files": 1, "max_matches": 1},
            "operation": {"provider": "text", "operation": {"type": "replace", "target": "old", "replacement": "new"}}}


def prepare(case):
    if case not in CASES:
        raise ValueError("unknown fixture")
    Path(".hostile").mkdir(exist_ok=True)
    data = b"old\nuntouched\n"
    path = "target.txt"
    r = request()
    if case in ("ambiguous-exact-match", "candidate-guard-staleness"):
        data = b"old\nkeep\nold\n"
    if case == "crlf-preservation":
        data = b'{\r\n  "name": "old",\r\n  "keep": "\u03bb"\r\n}\r\n'
        path = "target.json"
        r = request(path)
        r["operation"] = {"provider": "json", "operation": {"type": "set", "path": "$.name", "value": "new"}}
    if case == "unicode-bom":
        data = b"\xef\xbb\xbf" + "old\n\u03bb \U0001f987 caf\u00e9\n".encode("utf-8")
    if case == "hard-budget-overrun":
        r["budget"]["max_changed_bytes"] = 0
    if case == "replayed-identical-request":
        path = "target.json"
        data = b'{"name":"old","keep":1}\n'
        r = request(path)
        r["operation"] = {"provider": "json", "operation": {"type": "set", "path": "$.name", "value": "new"}}
    if case == "unknown-fields-strictness":
        r["unexpected"] = True
        nested = request()
        nested["operation"]["operation"]["unexpected"] = True
        write_json(".hostile/request2.json", nested)
        budget = request()
        budget["budget"]["unexpected"] = True
        write_json(".hostile/request3.json", budget)
    if case == "malformed-structured-no-fallback":
        path = "target.json"
        data = b'{"name":"old", BROKEN\n'
        r = request(path)
        r["operation"] = {"provider": "json", "operation": {"type": "set", "path": "$.name", "value": "new"}}
    if case == "path-prefix-escape":
        Path("allowed").mkdir()
        path = "outside.txt"
        r = request(path)
        r["budget"]["allowed_path_prefixes"] = ["allowed"]
        traversal = json.loads(json.dumps(r))
        traversal["file_path"] = "allowed/../outside.txt"
        write_json(".hostile/request2.json", traversal)
    if case == "junction-symlink-escape":
        Path("allowed").mkdir()
        Path("outside").mkdir()
        path = "outside/target.txt"
        r = request("allowed/link/target.txt")
        r["budget"]["allowed_path_prefixes"] = ["allowed"]
        try:
            if os.name == "nt":
                result = subprocess.run(["cmd", "/c", "mklink", "/J", str(Path("allowed/link").absolute()),
                                         str(Path("outside").absolute())], capture_output=True)
                if result.returncode:
                    raise OSError(result.stderr.decode("utf-8", "replace"))
            else:
                Path("allowed/link").symlink_to(Path("outside").absolute(), target_is_directory=True)
        except OSError as error:
            write_json(".hostile/unavailable.json", {"reason": "junction/symlink setup unsupported: " + str(error)})
    Path(path).write_bytes(data)
    Path(".hostile/before.bin").write_bytes(data)
    write_json(".hostile/state.json", {"case": case, "path": path})
    write_json(".hostile/request.json", r)
    if case == "transaction-rollback":
        Path("second.txt").write_bytes(b"second unchanged\n")
        later = request("second.txt")
        later["operation"]["operation"]["target"] = "absent"
        write_json(".hostile/request.json", {"version": "1.3.1", "transaction_id": "hostile-rollback",
                                            "requests": [r, later]})
    print(json.dumps({"prepared": case}))


def intervene(case):
    state = json.loads(Path(".hostile/state.json").read_text())
    if case in ("stale-preview", "stale-plan", "candidate-guard-staleness"):
        content = Path(state["path"]).read_bytes() + b"intervening\n"
        Path(state["path"]).write_bytes(content)
        Path(".hostile/before.bin").write_bytes(content)
    if case in ("stale-preview", "candidate-guard-staleness"):
        wrapper = json.loads(Path(".hostile/first.json").read_text())
        certificate = wrapper["document"]
        r = json.loads(Path(".hostile/request.json").read_text())
        r["expected_pre_hash"] = certificate["pre_hash"]
        if case == "candidate-guard-staleness":
            candidates = certificate["refusal_reason"]["duplicate_target"]["candidates"]
            r["candidate_guard"] = {"offset": candidates[0]["offset"], "selection_id": candidates[0]["selection_id"]}
            refreshed = json.loads(json.dumps(r))
            refreshed["expected_pre_hash"] = hashlib.sha256(content).hexdigest()
            write_json(".hostile/request2.json", refreshed)
        write_json(".hostile/request.json", r)
    if case in ("stale-plan", "tampered-plan"):
        plan = json.loads(Path(".hostile/first.json").read_text())["document"]
        if case == "tampered-plan":
            plan["operations"][0]["request"]["operation"]["operation"]["replacement"] = "tampered"
        write_json(".hostile/plan.json", plan)
    print(json.dumps({"intervened": case}))


def check(case):
    if Path(".hostile/unavailable.json").exists():
        return {"valid": False, **json.loads(Path(".hostile/unavailable.json").read_text())}
    state = json.loads(Path(".hostile/state.json").read_text())
    if state["case"] != case:
        raise ValueError("fixture identity mismatch")
    before = Path(".hostile/before.bin").read_bytes()
    after = Path(state["path"]).read_bytes()
    wrappers = [json.loads(p.read_text(encoding="utf-8")) for p in sorted(Path(".hostile").glob("result*.json"))]
    if not wrappers:
        raise ValueError("no Threadmoth result artifact")
    documents = [w["document"] for w in wrappers]
    def unwrap(doc):
        if isinstance(doc, dict) and doc.get("kind") == "certificate":
            doc = {k: v for k, v in doc.items() if k != "kind"}
        return doc
    documents = [unwrap(d) for d in documents]
    if any(not isinstance(d, dict) or "outcome" not in d for d in documents):
        return {"valid": False, "reason": "missing certificate outcome"}
    outcomes = [d.get("outcome") for d in documents]
    reasons = [d.get("reason_code") for d in documents]
    exits = [w["threadmoth_exit_code"] for w in wrappers]
    facts = {"valid": True, "case": case, "before_sha256": hashlib.sha256(before).hexdigest(),
             "after_sha256": hashlib.sha256(after).hexdigest(), "outcomes": outcomes,
             "reason_codes": reasons, "exit_codes": exits}
    # Unavailable/malformed prerequisites provide no violation predicate.
    if any(not isinstance(d, dict) or "outcome" not in d for d in documents):
        return {**facts, "valid": False, "reason": "missing certificate outcome"}
    refusing = ("stale-preview", "stale-plan", "tampered-plan", "path-prefix-escape",
                "junction-symlink-escape", "ambiguous-exact-match", "candidate-guard-staleness",
                "hard-budget-overrun", "unknown-fields-strictness", "malformed-structured-no-fallback",
                "transaction-rollback")
    if case in refusing:
        violation = after != before or any(o != "REFUSED" for o in outcomes) or any(x != 2 for x in exits)
        typed = {"stale-preview": ("STALE_IDENTITY",), "stale-plan": ("PLAN_STALE",),
                 "tampered-plan": ("PLAN_INVALID",), "ambiguous-exact-match": ("TARGET_AMBIGUOUS",),
                 "candidate-guard-staleness": ("STALE_IDENTITY",)}
        if case in typed:
            if case == "candidate-guard-staleness":
                violation |= reasons != ["STALE_IDENTITY", "CANDIDATE_SELECTION_INVALID"]
            else:
                violation |= any(code not in typed[case] for code in reasons)
        if case == "transaction-rollback":
            violation |= Path("second.txt").read_bytes() != b"second unchanged\n"
    elif case == "replayed-identical-request":
        first = json.loads(Path(".hostile/first.json").read_text())["document"]
        first_bytes = Path(".hostile/after-first.bin").read_bytes()
        violation = outcomes != ["NO_CHANGE"] or after != first_bytes or first.get("outcome") != "APPLIED"
    else:
        if case == "crlf-preservation":
            expected = before.replace(b'"old"', b'"new"')
        else:
            expected = before.replace(b"old", b"new", 1)
        doc = documents[-1]
        facts["certificate_post_hash"] = doc.get("post_hash")
        facts["changed_ranges"] = doc.get("changed_ranges")
        violation = outcomes != ["APPLIED"] or exits != [0] or after != expected
        violation |= doc.get("post_hash") != hashlib.sha256(after).hexdigest()
        if case in ("crlf-preservation", "unicode-bom"):
            preservation = doc.get("preservation", {})
            violation |= any(preservation.get(k) is not False for k in ("unrelated_bytes_changed", "line_endings_changed", "bom_changed"))
        if case == "certificate-byte-truth":
            ranges = doc.get("changed_ranges", [])
            # This same-width edit changes exactly offsets 0..3; certificate must cover every changed byte.
            changed = [i for i, (a, b) in enumerate(zip(before, after)) if a != b]
            violation |= not changed or any(not any(r["start"] <= i < r["end"] for r in ranges) for i in changed)
    facts["violation"] = bool(violation)
    return facts


def main():
    operation, case = sys.argv[1:3]
    try:
        if operation == "prepare":
            prepare(case)
        elif operation == "intervene":
            intervene(case)
        elif operation == "save-first":
            state = json.loads(Path(".hostile/state.json").read_text())
            Path(".hostile/after-first.bin").write_bytes(Path(state["path"]).read_bytes())
        elif operation == "check":
            facts = check(case)
            write_json(".hostile/observation.json", facts)
            print(json.dumps(facts))
        else:
            raise ValueError("unknown fixture operation")
    except (OSError, ValueError, KeyError, TypeError, IndexError) as error:
        print(json.dumps({"valid": False, "reason": str(error)}))
        sys.exit(2)


if __name__ == "__main__":
    main()
