//! Evidence store tests: content addressing, dedup, corruption detection,
//! atomicity, operation log ordering, and time helpers.

mod common;

use common::TempDir;
use terrorbat::evidence::{EvidenceStore, OperationLog, rfc3339_utc};

#[test]
fn put_get_roundtrip_and_dedup() {
    let dir = TempDir::new("ev-roundtrip");
    let store = EvidenceStore::open(&dir.join("store")).expect("open store");
    let a = store.put(b"hello evidence").expect("put a");
    let b = store.put(b"hello evidence").expect("put b");
    assert_eq!(a, b, "identical bytes must resolve to one object");
    assert!(a.0.starts_with("evidence:sha256:"));
    let back = store.get(&a).expect("get");
    assert_eq!(back, b"hello evidence");
    // Exactly one object file for the deduplicated content.
    let objects = dir.join("store").join("objects");
    let count = walkdir_count(&objects);
    let c = store.put(b"different bytes").expect("put c");
    assert_ne!(a, c);
    assert_eq!(walkdir_count(&objects), count + 1);
}

fn walkdir_count(root: &std::path::Path) -> usize {
    let mut n = 0;
    let mut stack = vec![root.to_path_buf()];
    while let Some(p) = stack.pop() {
        if let Ok(entries) = std::fs::read_dir(&p) {
            for e in entries.flatten() {
                if e.path().is_dir() {
                    stack.push(e.path());
                } else {
                    n += 1;
                }
            }
        }
    }
    n
}

#[test]
fn corrupt_object_is_detected_and_never_trusted() {
    let dir = TempDir::new("ev-corrupt");
    let root = dir.join("store");
    let store = EvidenceStore::open(&root).expect("open");
    let r = store.put(b"trusted bytes").expect("put");
    // Corrupt the stored object behind the store's back.
    let hex = r.hex().expect("hex");
    let obj = root
        .join("objects")
        .join("sha256")
        .join(&hex[..2])
        .join(&hex[2..]);
    std::fs::write(&obj, b"tampered bytes!").expect("tamper");
    let err = store.get(&r).expect_err("corrupt get must fail");
    assert!(err.to_string().contains("CORRUPT"), "{err}");
    // Re-putting the original bytes must also refuse: the existing object
    // no longer matches its digest.
    let err2 = store
        .put(b"trusted bytes")
        .expect_err("corrupt put must fail");
    assert!(err2.to_string().contains("CORRUPT"), "{err2}");
}

#[test]
fn malformed_refs_are_rejected() {
    let dir = TempDir::new("ev-refs");
    let store = EvidenceStore::open(&dir.join("store")).expect("open");
    for bad in [
        "evidence:sha256:tooshort",
        "evidence:md5:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "nonsense",
    ] {
        let r = terrorbat::evidence::EvidenceRef(bad.to_string());
        let err = store.get(&r).expect_err("must reject");
        assert!(err.to_string().contains("malformed"), "{bad}: {err}");
    }
    // Well-formed but absent.
    let absent = terrorbat::evidence::EvidenceRef::from_hex(&"a".repeat(64));
    let err = store.get(&absent).expect_err("absent");
    assert!(err.to_string().contains("not found"), "{err}");
}

#[test]
fn run_dirs_are_unique_and_listable() {
    let dir = TempDir::new("ev-runs");
    let store = EvidenceStore::open(&dir.join("store")).expect("open");
    let d1 = store.create_run_dir("exec-1").expect("create");
    assert!(d1.exists());
    let err = store.create_run_dir("exec-1").expect_err("duplicate");
    assert!(err.to_string().contains("already exists"), "{err}");
    store.create_run_dir("exec-0").expect("create");
    let mut runs = store.list_runs();
    runs.sort();
    assert_eq!(runs, vec!["exec-0".to_string(), "exec-1".to_string()]);
}

#[test]
fn operation_log_orders_and_resumes() {
    let dir = TempDir::new("ev-oplog");
    let run_dir = dir.join("run");
    std::fs::create_dir_all(&run_dir).expect("run dir");
    {
        let mut log = OperationLog::create(&run_dir).expect("create log");
        log.record("step_start", serde_json::json!({"index": 0}))
            .expect("record");
        log.record("step_finish", serde_json::json!({"index": 0}))
            .expect("record");
    }
    {
        let mut log = OperationLog::open_existing(&run_dir).expect("reopen");
        log.record("cleanup", serde_json::json!({}))
            .expect("record after reopen");
    }
    let text = std::fs::read_to_string(run_dir.join("operations.jsonl")).expect("read log");
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), 3);
    let seqs: Vec<u64> = lines
        .iter()
        .map(|l| {
            serde_json::from_str::<serde_json::Value>(l).unwrap()["seq"]
                .as_u64()
                .unwrap()
        })
        .collect();
    assert_eq!(
        seqs,
        vec![1, 2, 3],
        "sequence must be monotonic across reopen"
    );
    // Every line is valid JSON with a timestamp.
    for line in lines {
        let v: serde_json::Value = serde_json::from_str(line).expect("valid jsonl");
        assert!(v["unix_ms"].as_u64().is_some());
        assert!(v["event"].as_str().is_some());
    }
}

#[test]
fn self_test_round_trips() {
    let dir = TempDir::new("ev-selftest");
    let store = EvidenceStore::open(&dir.join("store")).expect("open");
    store.self_test().expect("self test");
}

#[test]
fn rfc3339_renders_known_timestamps() {
    assert_eq!(rfc3339_utc(0), "1970-01-01T00:00:00.000Z");
    assert_eq!(rfc3339_utc(1_767_225_600_123), "2026-01-01T00:00:00.123Z");
    // Regression for the month/minute shadowing bug: minute must not render
    // as the month.
    let ts = 1_767_225_600_000 + (45 * 60 + 7) * 1000; // 00:45:07
    assert_eq!(rfc3339_utc(ts), "2026-01-01T00:45:07.000Z");
}
