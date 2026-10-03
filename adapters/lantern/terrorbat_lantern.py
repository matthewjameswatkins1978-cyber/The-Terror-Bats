#!/usr/bin/env python3
"""Thin one-shot M8 adapter around Lantern's real contract binaries and exporter.
No model decisions and no production service/store access.
"""
import argparse, hashlib, json, os, subprocess, sys
from pathlib import Path
from checker import ledger
from mirror_fixture import run as mirror

PROTOCOL="terrorbat-adapter/v1"
CONTRACTS={
 "rewritten_history":"bridge_github::tests::test_queue_ancestry_verification_and_suspect_on_rewritten_history",
 "modified_intent":"bridge_github::tests::test_queue_modifying_intent_is_suspect",
 "oldest_first":"bridge_github::tests::test_queue_oldest_first_ordering",
 "authority":"bridge_github::tests::hostile_authority_history",
 "observed_hash":"bridge_github::tests::test_executor_observed_hash_precondition_conflict",
 "crash_replay_tamper":"bridge_github::tests::test_executor_idempotency_crash_seam_and_tamper",
 "checkpoint_reopen":"bridge_github::state::tests::state_save_and_reload",
 "applied_reopen":"bridge_github::state::tests::crash_seam_applied_mutation_survives_restart",
 "restarted_cycles":"bridge_github::tests::hostile_restarted_cycles_history",
 "reinforce_disabled":"bridge_github::tests::test_memory_reinforce_is_explicitly_disabled",
}


def execute(args, request):
    payload=request["payload"];action=request["action"]
    context=request["context"]
    if context["target_commit"] != args.revision:
        raise ValueError("Bat target commit differs from explicitly bound Lantern fixture revision")
    target=Path(context["worktree"])
    report={"lantern_product_revision":args.product_revision,"lantern_fixture_revision":args.revision,"action":action,"execution_id":context["execution_id"]}
    if action=="ledger_history":
        mode=payload.get("scenario");writers=payload.get("writers")
        if mode not in {"race","sequential","different"} or type(writers) is not int or not 1<=writers<=32: raise ValueError("invalid bounded ledger payload")
        env=os.environ.copy();env.update(TB_LANTERN_SCENARIO=mode,TB_LANTERN_WRITERS=str(writers))
        binary=Path(args.ledger_binary)
        result=subprocess.run([str(binary),"--exact","emit_history","--nocapture","--test-threads=1"],cwd=target,env=env,capture_output=True,timeout=100)
        output=result.stdout.decode("utf-8","replace");diagnostics=result.stderr.decode("utf-8","replace")
        if result.returncode!=0: raise RuntimeError(f"ledger fixture did not complete: {output}\n{diagnostics}")
        lines=[line.split("TB_HISTORY=",1)[1] for line in output.splitlines() if "TB_HISTORY=" in line]
        if len(lines)!=1: raise RuntimeError("expected exactly one complete ledger history")
        history=json.loads(lines[0]);report.update(history=history,checks=ledger(history),fixture_stdout=output,fixture_stderr=diagnostics)
    elif action=="bridge_contract":
        scenario=payload.get("scenario")
        if scenario not in CONTRACTS: raise ValueError("unknown bridge contract")
        binary=Path(args.bridge_binary)
        result=subprocess.run([str(binary),"--exact",CONTRACTS[scenario],"--nocapture","--test-threads=1"],cwd=target,capture_output=True,timeout=100)
        output=result.stdout.decode("utf-8","replace");diagnostics=result.stderr.decode("utf-8","replace")
        if result.returncode!=0: raise RuntimeError(f"bridge contract failed or did not complete; inspect raw assertion/setup evidence: {output}\n{diagnostics}")
        if "1 passed; 0 failed" not in output: raise RuntimeError("bridge contract filter did not execute one real test")
        report.update(scenario=scenario,contract_test=CONTRACTS[scenario],fixture_stdout=output,fixture_stderr=diagnostics,checks={"violation":False,"passed":1})
        if scenario=="authority":
            lines=[line.split("TB_BRIDGE_HISTORY=",1)[1] for line in output.splitlines() if "TB_BRIDGE_HISTORY=" in line]
            if len(lines)!=1: raise RuntimeError("missing complete authority history")
            history=json.loads(lines[0]); rows=history["history"]
            if [r["actor"] for r in rows]!=["allow","ask","deny","unavailable"]:raise ValueError("incomplete authority outcomes")
            expected={"allow":"APPLIED","ask":"REQUIRES_APPROVAL","deny":"DENIED","unavailable":"DENIED"}
            report.update(history=history,checks={"violation":any(r["after_calls"]-r["before_calls"]!=(1 if r["actor"]=="allow" else 0) or r["receipt"]["status"]!=expected[r["actor"]] for r in rows)})
        if scenario=="restarted_cycles":
            lines=[line.split("TB_BRIDGE_HISTORY=",1)[1] for line in output.splitlines() if "TB_BRIDGE_HISTORY=" in line]
            if len(lines)!=1:raise RuntimeError("missing complete runner history")
            history=json.loads(lines[0]);cycles=history["cycles"]
            if len(cycles)!=3 or not all(c["completed"] for c in cycles):raise RuntimeError("runner cycles did not complete: "+json.dumps(history))
            expected=history["expected_intents"]
            actual=[m["content"] for m in history["mutations"]]
            checkpoint_bad=actual!=expected or any(c["reopened_state"]["last_processed_inbox_commit"]!=history["inbox_head"] or sorted(c["reopened_state"]["idempotency"])!=sorted(expected) for c in cycles)
            repeat_bad=cycles[0]["after_calls"]!=len(expected) or any(c["before_calls"]!=len(expected) or c["after_calls"]!=len(expected) for c in cycles[1:])
            receipts_bad=sorted(r["intent_id"] for r in history["receipts"])!=sorted(expected) or any(r["status"]!="APPLIED" for r in history["receipts"]) or history["status"]["last_processed_inbox_commit"]!=history["inbox_head"] or history["status"]["pending_count"]!=0
            report.update(history=history,checks={"violation":checkpoint_bad or repeat_bad or receipts_bad,"checkpoint_violation":checkpoint_bad or receipts_bad,"repeated_cycle_violation":repeat_bad or receipts_bad})
    elif action=="mirror_probe":
        scenario=payload.get("scenario")
        if scenario not in {"normal","crlf","atomic"}:raise ValueError("invalid mirror scenario")
        report.update(history=mirror(target,scenario));report["checks"]={"violation":report["history"]["violation"]}
        binary=target/"scripts/lantern_git_export.py"
    else: raise ValueError("unknown adapter action")
    report["target_surface"]={"path":str(binary),"sha256":hashlib.sha256(binary.read_bytes()).hexdigest()}
    return report


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ledger-binary",required=True);parser.add_argument("--bridge-binary",required=True)
    parser.add_argument("--revision",required=True);parser.add_argument("--product-revision",required=True)
    args=parser.parse_args()
    request=json.loads(sys.stdin.readline())
    if request.get("protocol")!=PROTOCOL:raise ValueError("unsupported protocol")
    source_id=hashlib.sha256(b''.join((Path(__file__).parent/name).read_bytes() for name in ('terrorbat_lantern.py','checker.py','mirror_fixture.py'))).hexdigest()[:16]
    if request.get('kind')=='describe':
        response={"protocol":PROTOCOL,"kind":"description","name":"lantern","version":"0.1.0+"+args.revision+"."+source_id,"actions":{name:{"requires":["fs.read","fs.write","git.inspect","network"]} for name in ("ledger_history","bridge_contract","mirror_probe")}}
    else:
        response={"protocol":PROTOCOL,"kind":"result","request_id":request["request_id"],"status":"completed","exit_code":0,"stdout":"","stderr":""}
        try:response["stdout"]=json.dumps(execute(args,request),sort_keys=True)
        except Exception as error:
            response.update(status="infrastructure_error",exit_code=2,stderr=f"{type(error).__name__}: {error}")
    print(json.dumps(response,sort_keys=True))

if __name__=="__main__":main()