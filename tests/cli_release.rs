//! CLI release polish regressions (Phase 5): completions for the four
//! required shells, the generated man page, the exact version string, and
//! a machine-readable doctor report. All surfaces derive from the live
//! Clap definition — never hand-maintained copies.
//!
//! The docs smoke test executes the manual's First Flight loop
//! (run → inspect → evidence → replay) through the real binary so the
//! documented commands cannot become fan fiction.

use std::process::Command;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_terrorbats"))
}

fn run(args: &[&str]) -> (i32, String) {
    let output = cli().args(args).output().expect("terrorbats binary runs");
    let code = output.status.code().unwrap_or(-1);
    (code, String::from_utf8_lossy(&output.stdout).to_string())
}

fn repo_with_commit(root: &std::path::Path) -> std::path::PathBuf {
    let repo = root.join("target");
    std::fs::create_dir_all(&repo).expect("repo dir");
    let git = |args: &[&str]| {
        let status = Command::new("git")
            .args(args)
            .current_dir(&repo)
            .status()
            .expect("git runs");
        assert!(status.success(), "git {args:?} must succeed");
    };
    git(&["init"]);
    git(&["config", "user.email", "smoke@test.local"]);
    git(&["config", "user.name", "Smoke"]);
    git(&["commit", "--allow-empty", "-m", "smoke"]);
    repo
}

#[test]
fn manual_first_flight_loop_runs_end_to_end() {
    // Every command in docs/MANUAL.md §5/§17–§19, executed for real.
    let root = std::env::temp_dir().join(format!("terrorbats-smoke-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("smoke dir");
    let repo = repo_with_commit(&root);
    let store = root.join("store");
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let bat = manifest.join("bats").join("command-exit.yaml");
    // run
    let run_out = cli()
        .arg("run")
        .arg(&bat)
        .arg("--repo")
        .arg(&repo)
        .arg("--store")
        .arg(&store)
        .arg("--json")
        .output()
        .expect("run executes");
    assert!(run_out.status.success(), "first flight run must succeed");
    let receipt: serde_json::Value = serde_json::from_slice(&run_out.stdout).expect("receipt JSON");
    assert_eq!(receipt["verdict"].as_str(), Some("NOT OBSERVED"));
    let id = receipt["receipt_id"]
        .as_str()
        .expect("receipt id")
        .to_string();
    // inspect
    let inspect = cli()
        .arg("inspect")
        .arg(&id)
        .arg("--store")
        .arg(&store)
        .output()
        .expect("inspect executes");
    assert!(inspect.status.success(), "inspect must succeed");
    // evidence show, from the first step's stdout reference
    let stdout_ref = receipt["execution"]["steps"][0]["stdout"]
        .as_str()
        .expect("stdout evidence ref")
        .to_string();
    let evidence = cli()
        .arg("evidence")
        .arg("show")
        .arg(&stdout_ref)
        .arg("--store")
        .arg(&store)
        .output()
        .expect("evidence show executes");
    assert!(evidence.status.success(), "evidence show must succeed");
    assert!(!evidence.stdout.is_empty(), "evidence must print bytes");
    // replay
    let replay = cli()
        .arg("replay")
        .arg(&id)
        .arg("--store")
        .arg(&store)
        .output()
        .expect("replay executes");
    assert!(replay.status.success(), "replay must succeed");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn completions_cover_required_shells() {
    for shell in ["powershell", "bash", "zsh", "fish"] {
        let (code, stdout) = run(&["completions", shell]);
        assert_eq!(code, 0, "{shell} completions must exit 0");
        assert!(
            stdout.contains("terrorbats"),
            "{shell} completions must reference the binary"
        );
        assert!(
            stdout.lines().count() > 5,
            "{shell} completions must be a real script, not a stub"
        );
    }
}

#[test]
fn man_page_comes_from_live_definition() {
    let (code, stdout) = run(&["man"]);
    assert_eq!(code, 0, "man must exit 0");
    assert!(stdout.contains(".TH"), "man output must be roff");
    assert!(
        stdout.contains("terrorbats"),
        "man page must name the binary"
    );
    assert!(
        stdout.contains(env!("CARGO_PKG_VERSION")),
        "man page must carry the release version"
    );
}

#[test]
fn version_output_is_exact() {
    let (code, stdout) = run(&["--version"]);
    assert_eq!(code, 0);
    assert_eq!(
        stdout.trim(),
        format!("terrorbats {}", env!("CARGO_PKG_VERSION")),
        "version string must be exact"
    );
}

#[test]
fn doctor_json_reports_required_sections() {
    let (code, stdout) = run(&["doctor", "--json"]);
    assert_eq!(code, 0, "doctor must be healthy on a dev/CI machine");
    let report: serde_json::Value =
        serde_json::from_str(&stdout).expect("doctor --json must be JSON");
    for key in [
        "terrorbat_version",
        "core",
        "optional_tools",
        "isolation",
        "overall_ready",
    ] {
        assert!(
            report.get(key).is_some(),
            "doctor report must contain `{key}`"
        );
    }
    assert_eq!(
        report["terrorbat_version"].as_str(),
        Some(env!("CARGO_PKG_VERSION")),
        "doctor must report the release version"
    );
    assert_eq!(
        report["overall_ready"].as_bool(),
        Some(true),
        "doctor must be ready on a dev/CI machine"
    );
}
