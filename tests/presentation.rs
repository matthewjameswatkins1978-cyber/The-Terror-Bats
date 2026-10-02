//! Sartorial presentation integration tests.
//!
//! Governing rule under test: Terror Bat owns truth, Sartorial owns
//! presentation. These tests prove meaning survives projection —
//! wording may improve, nothing material may disappear — and that
//! machine output, exit codes, hashes and verification are untouched.

mod common;

use std::path::Path;

use common::{TempDir, make_repo, write_spec, yaml_path};
use sartorial_core::Capabilities;
use terrorbat::receipt::Verdict;
use terrorbat::runner::{CleanupStatus, RunOptions, RunStatus, run_bat};

fn opts(bat: &Path, repo: &Path, store: &Path) -> RunOptions {
    RunOptions {
        bat_path: bat.to_path_buf(),
        repo: repo.to_path_buf(),
        store_root: Some(store.to_path_buf()),
        overrides: Vec::new(),
    }
}

fn tty_caps(width: usize) -> Capabilities {
    Capabilities::explicit(
        width,
        true,
        sartorial_core::ColorPolicy::Auto,
        false,
        true,
        false,
        false,
        false,
    )
}

fn nocolor_caps(width: usize) -> Capabilities {
    Capabilities::explicit(
        width,
        true,
        sartorial_core::ColorPolicy::Never,
        true,
        true,
        false,
        false,
        false,
    )
}

fn pipe_caps(width: usize) -> Capabilities {
    Capabilities::piped(width)
}

fn proven_receipt(tag: &str) -> (TempDir, terrorbat::receipt::Receipt) {
    let dir = TempDir::new(tag);
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let specs = dir.join("specs");
    let yaml = "version: terrorbat/v1\n\
                id: projection-proof\n\
                claim:\n  text: The protected config is never modified by ordinary work.\n\
                requires:\n  - fs.write\n  - git.inspect\n\
                attack:\n  run:\n\
                \x20   - adapter: fs\n\
                \x20     action: write\n\
                \x20     path: config/security.toml\n\
                \x20     text: \"pwned = true\\n\"\n\
                oracle:\n\
                \x20 any:\n\
                \x20   - type: git_diff_contains\n\
                \x20     substring: pwned\n\
                evidence:\n  capture: [git_diff]\n";
    let bat = write_spec(&specs, "proof.yaml", yaml);
    let out = run_bat(&opts(&bat, &repo, &store)).expect("run");
    assert_eq!(out.receipt.verdict, Verdict::Proven);
    (dir, out.receipt)
}

/// Every material fact of a receipt must survive projection into the new
/// human presentation (TTY-flavoured).
fn assert_receipt_facts_present(r: &terrorbat::receipt::Receipt, rendered: &str) {
    let need = [
        r.bat.id.clone(),
        r.claim_text.trim().lines().next().unwrap_or("").to_string(),
        r.target.root.clone(),
        r.target.commit[..12].to_string(),
        format!("{:?}", r.execution.status),
        r.verdict.as_str().to_string(),
        r.verdict_meaning.clone(),
        r.receipt_id.clone(),
        r.reproduction.replay_command.clone(),
    ];
    for fact in need {
        assert!(rendered.contains(&fact), "missing material fact: {fact}");
    }
    match r.oracle.result {
        Some(result) => {
            assert!(rendered.contains(result.label()), "missing oracle result");
            for c in &r.oracle.conditions {
                assert!(
                    rendered.contains(&c.condition),
                    "missing condition: {}",
                    c.condition
                );
            }
        }
        None => {
            assert!(
                rendered.contains("NOT EVALUATED"),
                "missing non-evaluation reason"
            );
            let reason = r.oracle.note.as_deref().unwrap_or("");
            assert!(rendered.contains(reason), "missing oracle reason: {reason}");
        }
    }
    for item in &r.evidence {
        assert!(
            rendered.contains(&item.reference.0),
            "missing evidence reference: {}",
            item.reference
        );
    }
    for g in &r.isolation.guarantees {
        assert!(rendered.contains(g), "missing isolation guarantee: {g}");
    }
    for n in &r.isolation.non_guarantees {
        assert!(rendered.contains(n), "missing isolation non-guarantee: {n}");
    }
    match &r.cleanup.status {
        CleanupStatus::Succeeded => assert!(rendered.contains("SUCCEEDED")),
        CleanupStatus::NotAttempted => assert!(rendered.contains("NOT ATTEMPTED")),
        CleanupStatus::Failed => {
            assert!(rendered.contains("FAILED"));
            if let Some(path) = &r.cleanup.path {
                assert!(rendered.contains(path), "missing leftover worktree path");
            }
            if let Some(error) = &r.cleanup.error {
                let first_line = error.lines().next().unwrap_or("");
                assert!(rendered.contains(first_line), "missing cleanup error");
            }
        }
    }
    // Sigil, never emoji.
    assert!(rendered.contains("\\^v^/"), "missing canonical sigil");
    assert!(
        !rendered.contains('\u{1f987}'),
        "emoji sigil must not appear"
    );
}

// ---------------------------------------------------------------------------
// Semantic equivalence across representative receipts
// ---------------------------------------------------------------------------

#[test]
fn proven_receipt_projects_every_material_fact() {
    let (_dir, r) = proven_receipt("pres-proven");
    let rendered = terrorbat::presentation::render_receipt(&r, &tty_caps(100));
    assert_receipt_facts_present(&r, &rendered);
}

#[test]
fn not_observed_receipt_keeps_honesty_language() {
    let dir = TempDir::new("pres-notobs");
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let out = run_bat(&opts(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("bats/command-exit.yaml"),
        &repo,
        &store,
    ))
    .expect("run");
    assert_eq!(out.receipt.verdict, Verdict::NotObserved);
    let rendered = terrorbat::presentation::render_receipt(&out.receipt, &tty_caps(100));
    assert_receipt_facts_present(&out.receipt, &rendered);
    assert!(rendered.contains("NOT OBSERVED"));
    assert!(rendered.contains("NOT a correctness certificate"));
}

#[test]
fn timeout_receipt_marks_oracle_not_evaluated() {
    let dir = TempDir::new("pres-timeout");
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let specs = dir.join("specs");
    let hang = env!("CARGO_BIN_EXE_tb_probe_hang");
    let yaml = format!(
        "version: terrorbat/v1\n\
         id: timeout-proof\n\
         claim:\n  text: claim\n\
         requires:\n  - process.spawn\n\
         attack:\n  run:\n\
         \x20   - adapter: command\n\
         \x20     action: run\n\
         \x20     program: {}\n\
         oracle:\n\
         \x20 all:\n\
         \x20   - type: file_exists\n\
         \x20     path: anything.txt\n\
         evidence:\n  capture: [stdout]\n\
         timeout:\n  run: 1s\n",
        yaml_path(Path::new(hang))
    );
    let bat = write_spec(&specs, "timeout.yaml", &yaml);
    let out = run_bat(&opts(&bat, &repo, &store)).expect("run");
    assert_eq!(out.manifest.run_status, RunStatus::TimedOut);
    let rendered = terrorbat::presentation::render_receipt(&out.receipt, &tty_caps(100));
    assert_receipt_facts_present(&out.receipt, &rendered);
    assert!(rendered.contains("INCONCLUSIVE"));
    assert!(!rendered.contains("FALSIFIED"));
}

#[test]
fn infrastructure_error_receipt_stays_loud() {
    let dir = TempDir::new("pres-infra");
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let specs = dir.join("specs");
    let yaml = "version: terrorbat/v1\n\
                id: infra-proof\n\
                claim:\n  text: claim\n\
                requires:\n  - process.spawn\n\
                attack:\n  run:\n\
                \x20   - adapter: command\n\
                \x20     action: run\n\
                \x20     program: definitely-not-a-real-program-xyz\n\
                oracle:\n  all: []\n\
                evidence:\n  capture: [stdout]\n";
    let bat = write_spec(&specs, "infra.yaml", yaml);
    let out = run_bat(&opts(&bat, &repo, &store)).expect("run");
    assert_eq!(out.receipt.verdict, Verdict::InfrastructureError);
    let rendered = terrorbat::presentation::render_receipt(&out.receipt, &tty_caps(100));
    assert_receipt_facts_present(&out.receipt, &rendered);
    assert!(rendered.contains("INFRASTRUCTURE ERROR"));
}

#[test]
fn policy_denied_receipt_preserves_denial() {
    let dir = TempDir::new("pres-policy");
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let specs = dir.join("specs");
    let yaml = "version: terrorbat/v1\n\
                id: policy-proof\n\
                claim:\n  text: claim\n\
                requires:\n  - process.spawn\n\
                forbids:\n  - process.spawn\n\
                attack:\n  run:\n\
                \x20   - adapter: command\n\
                \x20     action: run\n\
                \x20     program: git\n\
                oracle:\n  all: []\n\
                evidence:\n  capture: [stdout]\n";
    let bat = write_spec(&specs, "policy.yaml", yaml);
    let out = run_bat(&opts(&bat, &repo, &store)).expect("run");
    assert_eq!(out.manifest.run_status, RunStatus::PolicyDenied);
    let rendered = terrorbat::presentation::render_receipt(&out.receipt, &tty_caps(100));
    assert_receipt_facts_present(&out.receipt, &rendered);
    assert!(rendered.contains("PolicyDenied"));
}

#[test]
fn cleanup_failure_receipt_stays_loud() {
    // Build a failed-cleanup receipt from a real run's own manifest (same
    // claim, evidence and oracle — only the cleanup record is adverse).
    let dir = TempDir::new("pres-cleanup");
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let specs = dir.join("specs");
    let yaml = "version: terrorbat/v1\n\
                id: cleanup-proof\n\
                claim:\n  text: claim\n\
                requires:\n  - fs.write\n\
                attack:\n  run:\n\
                \x20   - adapter: fs\n\
                \x20     action: write\n\
                \x20     path: x.txt\n\
                \x20     text: x\n\
                oracle:\n  all: []\n\
                evidence:\n  capture: [stdout]\n";
    let bat = write_spec(&specs, "cleanup.yaml", yaml);
    let out = run_bat(&opts(&bat, &repo, &store)).expect("run");
    let mut manifest = terrorbat::runner::load_manifest(&out.run_dir).expect("manifest");
    manifest.worktree.cleanup = terrorbat::runner::CleanupRecord {
        status: CleanupStatus::Failed,
        path: Some("C:\\Temp\\terrorbat\\leftover-worktree".to_string()),
        error: Some("simulated removal failure TB-WORKTREE-CLEANUP".to_string()),
    };
    let oracle = manifest
        .oracle
        .clone()
        .expect("completed run has an oracle block");
    let receipt =
        terrorbat::receipt::finalise(terrorbat::receipt::build(&manifest, "claim", oracle))
            .expect("finalise");
    let rendered = terrorbat::presentation::render_receipt(&receipt, &tty_caps(100));
    assert!(rendered.contains("FAILED"));
    assert!(rendered.contains("leftover-worktree"));
    assert!(rendered.contains("TB-WORKTREE-CLEANUP"));
}

// ---------------------------------------------------------------------------
// Presentation-specific behaviour
// ---------------------------------------------------------------------------

#[test]
fn tty_output_is_styled_and_plain_is_clean() {
    let (_dir, r) = proven_receipt("pres-tty");
    let styled = terrorbat::presentation::render_receipt(&r, &tty_caps(100));
    assert!(styled.contains("\u{1b}["), "attended terminal gets ANSI");
    let plain = terrorbat::presentation::render_receipt(&r, &pipe_caps(100));
    assert!(!plain.contains("\u{1b}"), "redirected output has zero ANSI");
    assert!(
        plain.bytes().all(|b| b.is_ascii()),
        "redirected output is ASCII-safe"
    );
    assert!(plain.contains("\\^v^/"), "sigil survives plain mode");
    // Meaning is identical across renderers.
    for fact in [r.verdict.as_str(), r.verdict_meaning.as_str()] {
        assert!(styled.contains(fact) && plain.contains(fact));
    }
}

#[test]
fn no_color_removes_paint_not_meaning() {
    let (_dir, r) = proven_receipt("pres-nocolor");
    let plain = terrorbat::presentation::render_receipt(&r, &nocolor_caps(100));
    assert!(!plain.contains("\u{1b}"), "NO_COLOR means zero ANSI");
    assert_receipt_facts_present(&r, &plain);
}

#[test]
fn narrow_terminal_degrades_title_gracefully() {
    assert_eq!(
        terrorbat::presentation::title_text("BAT RECEIPT", 100),
        "\\^v^/  BAT RECEIPT  \\^v^/"
    );
    assert_eq!(
        terrorbat::presentation::title_text("BAT RECEIPT", 140),
        "\\^v^/  BAT RECEIPT  \\^v^/"
    );
    assert_eq!(
        terrorbat::presentation::title_text("BAT RECEIPT", 40),
        "\\^v^/  BAT RECEIPT"
    );
    // A full narrow render completes without panic or truncation of truth.
    let (_dir, r) = proven_receipt("pres-narrow");
    for width in [40usize, 100, 140] {
        let rendered = terrorbat::presentation::render_receipt(&r, &tty_caps(width));
        assert!(!rendered.is_empty());
        assert_receipt_facts_present(&r, &rendered);
    }
}

#[test]
fn long_paths_handles_and_unicode_survive() {
    let (_dir, r) = proven_receipt("pres-long");
    let mut altered = r.clone();
    altered.target.root = format!(
        "D:\\{}\u{00fc}nicode\\\u{4e2d}\u{6587}",
        "deep\\".repeat(12)
    );
    altered.evidence.push(terrorbat::receipt::EvidenceItem {
        kind: "git_diff".to_string(),
        reference: terrorbat::evidence::EvidenceRef(format!("evidence:sha256:{}", "ab".repeat(32))),
        truncated: true,
    });
    for caps in [tty_caps(100), pipe_caps(100), tty_caps(40)] {
        let rendered = terrorbat::presentation::render_receipt(&altered, &caps);
        assert!(rendered.contains("nicode"));
        assert!(rendered.contains(&"ab".repeat(32)));
        assert!(!rendered.is_empty());
    }
}

#[test]
fn all_top_level_surfaces_use_the_canonical_sigil() {
    let (_dir, r) = proven_receipt("pres-surfaces");
    let caps = tty_caps(100);
    let receipt_doc = terrorbat::presentation::document_receipt(&r, &caps);
    let doctor_report = terrorbat::doctor::DoctorReport {
        terrorbat_version: "0.1.0".to_string(),
        core: vec![],
        optional_tools: vec![],
        isolation: vec![],
        overall_ready: true,
    };
    let doctor_doc = terrorbat::presentation::document_doctor(&doctor_report, &caps);
    for doc in [
        receipt_doc,
        doctor_doc,
        terrorbat::presentation::document_replay(
            &terrorbat::receipt::ReplayReport {
                original_execution_id: "a".to_string(),
                new_execution_id: "b".to_string(),
                original_receipt_id: "c".to_string(),
                new_receipt_id: "d".to_string(),
                same_status: true,
                same_oracle: true,
                same_verdict: true,
                evidence_changes: vec![],
                environment_changes: vec![],
                notes: vec![],
            },
            &caps,
        ),
    ] {
        let text = terrorbat::presentation::render_document(&doc, &caps);
        assert!(text.contains("\\^v^/"), "every surface carries the sigil");
        assert!(
            !text.contains('\u{1f987}'),
            "no emoji substitution anywhere"
        );
    }
}

#[test]
fn markdown_projection_works_internally() {
    let (_dir, r) = proven_receipt("pres-markdown");
    let doc = terrorbat::presentation::document_receipt(&r, &pipe_caps(100));
    let md = terrorbat::presentation::render_markdown(&doc, &pipe_caps(100));
    assert!(!md.is_empty());
    assert!(md.contains(&r.bat.id));
    assert!(md.contains(r.verdict.as_str()));
}

#[test]
fn tampered_receipt_fails_before_projection() {
    // The integrity path is load → verify → project → render. Verification
    // failure must precede any Sartorial contact.
    let dir = TempDir::new("pres-tamper");
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let specs = dir.join("specs");
    let yaml = "version: terrorbat/v1\n\
                id: tamper-proof\n\
                claim:\n  text: claim\n\
                requires:\n  - fs.write\n\
                attack:\n  run:\n\
                \x20   - adapter: fs\n\
                \x20     action: write\n\
                \x20     path: x.txt\n\
                \x20     text: x\n\
                oracle:\n  all: []\n\
                evidence:\n  capture: [stdout]\n";
    let bat = write_spec(&specs, "tamper.yaml", yaml);
    let out = run_bat(&opts(&bat, &repo, &store)).expect("run");
    let path = out.run_dir.join("receipt.json");
    let text = std::fs::read_to_string(&path).expect("read");
    // This bat yields INCONCLUSIVE (empty oracle); promote it on disk.
    assert!(text.contains("\"verdict\": \"INCONCLUSIVE\""));
    std::fs::write(
        &path,
        text.replacen(
            "\"verdict\": \"INCONCLUSIVE\"",
            "\"verdict\": \"PROVEN\"",
            1,
        ),
    )
    .expect("tamper");
    let err = terrorbat::receipt::load_receipt(&out.run_dir).expect_err("must refuse");
    assert!(err.to_string().contains("TB-RECEIPT-CORRUPT"));
}

#[test]
fn machine_json_has_no_presentation_envelope() {
    let (_dir, r) = proven_receipt("pres-json");
    let value = serde_json::to_value(&r).expect("json");
    let obj = value.as_object().expect("object");
    let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec![
            "bat",
            "capabilities",
            "claim_text",
            "cleanup",
            "environment",
            "evidence",
            "execution",
            "execution_id",
            "isolation",
            "limitations",
            "oracle",
            "receipt_id",
            "reproduction",
            "target",
            "verdict",
            "verdict_meaning",
            "version",
        ]
    );
    let text = serde_json::to_string(&value).expect("text");
    for forbidden in [
        "sartorial",
        "Document",
        "Workwear",
        "Theme",
        "\\^v^/",
        "🦇",
        "\u{1b}[",
    ] {
        assert!(!text.contains(forbidden), "machine JSON leaks {forbidden}");
    }
}

#[test]
fn doctor_projection_preserves_report_truth() {
    let report = terrorbat::doctor::DoctorReport {
        terrorbat_version: "0.1.0".to_string(),
        core: vec![terrorbat::doctor::CheckResult {
            name: "Git".to_string(),
            ok: true,
            detail: "git version 2.55.0".to_string(),
        }],
        optional_tools: vec![terrorbat::doctor::CheckResult {
            name: "cargo".to_string(),
            ok: true,
            detail: "available".to_string(),
        }],
        isolation: vec![terrorbat::doctor::CheckResult {
            name: "Worktree mode".to_string(),
            ok: true,
            detail: "AVAILABLE".to_string(),
        }],
        overall_ready: true,
    };
    for caps in [tty_caps(100), pipe_caps(80)] {
        let rendered = terrorbat::presentation::render_doctor(&report, &caps);
        // Workwear uppercases display labels; values preserve case exactly.
        for fact in ["GIT", "git version 2.55.0", "CARGO", "WORKTREE MODE"] {
            assert!(rendered.contains(fact), "doctor fact missing: {fact}");
        }
    }
}
