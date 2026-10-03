"""Sensitivity controls for the deterministic checker; these are not Lantern findings."""
import copy, unittest
from checker import ledger

class LedgerControls(unittest.TestCase):
    def healthy(self):
        record={"event_id":"fixture:one","idempotency_key":"key","content":"equivalent"}
        return {"schema":"lantern-hostile-history/v1","closed_and_reopened":True,"writers":2,
                "history":[{"writer":0,"status":"stored","record":record},{"writer":1,"status":"duplicate","record":record}],"durable_records":[record]}
    def test_equivalent_duplicates_healthy(self):self.assertFalse(ledger(self.healthy())["violation"])
    def test_error_is_violation(self):
        h=self.healthy();h["history"][1]={"writer":1,"status":"error","error":"unique storage conflict"};self.assertTrue(ledger(h)["violation"])
    def test_extra_durable_record_is_violation(self):
        h=self.healthy();h["durable_records"].append(copy.deepcopy(h["durable_records"][0]));self.assertTrue(ledger(h)["violation"])
    def test_incomplete_history_is_infrastructure(self):
        h=self.healthy();h["history"].pop()
        with self.assertRaises(ValueError):ledger(h)
    def test_missing_reopen_is_infrastructure(self):
        h=self.healthy();h["closed_and_reopened"]=False
        with self.assertRaises(ValueError):ledger(h)
    def test_missing_record_is_infrastructure(self):
        h=self.healthy();del h["history"][0]["record"]
        with self.assertRaises(ValueError):ledger(h)
    def test_divergent_duplicate_is_violation(self):
        h=self.healthy();h["history"][1]["record"]={"event_id":"other","idempotency_key":"key","content":"equivalent"};self.assertTrue(ledger(h)["violation"])

if __name__=="__main__":unittest.main()