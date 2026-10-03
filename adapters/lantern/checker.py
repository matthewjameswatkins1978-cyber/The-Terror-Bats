"""Deterministic factual checker. No PROVEN decisions, no target I/O."""
def ledger(history):
    if history.get("schema") != "lantern-hostile-history/v1" or history.get("closed_and_reopened") is not True:
        raise ValueError("missing complete close/reopen history")
    n = history["writers"]
    outcomes, records = history["history"], history["durable_records"]
    if not isinstance(n, int) or len(outcomes) != n or sorted(x["writer"] for x in outcomes) != list(range(n)):
        raise ValueError("incomplete competitor history")
    if not isinstance(records, list) or any(x.get("status") not in {"stored", "duplicate", "error"} for x in outcomes):
        raise ValueError("malformed target results")
    failures = [x for x in outcomes if x["status"] == "error"]
    healthy=[x for x in outcomes if x["status"] in {"stored","duplicate"}]
    if any(not isinstance(x.get('record'), dict) for x in healthy) or any(not isinstance(x, dict) for x in records):
        raise ValueError('missing factual record')
    convergence=len(records)==1 and all(x['record']==records[0] for x in healthy)
    one_winner=sum(x["status"] == "stored" for x in outcomes)==1
    return {"violation": not convergence or not one_winner or bool(failures), "durable_count": len(records),
            "writer_errors": len(failures), "stored": sum(x["status"] == "stored" for x in outcomes),
            "duplicates": sum(x["status"] == "duplicate" for x in outcomes)}