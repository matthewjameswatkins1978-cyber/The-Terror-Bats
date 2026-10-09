//! CLI release polish regressions (Phase 5): completions for the four
//! required shells, the generated man page, the exact version string, and
//! a machine-readable doctor report. All surfaces derive from the live
//! Clap definition — never hand-maintained copies.

use std::process::Command;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_terrorbats"))
}

fn run(args: &[&str]) -> (i32, String) {
    let output = cli().args(args).output().expect("terrorbats binary runs");
    let code = output.status.code().unwrap_or(-1);
    (code, String::from_utf8_lossy(&output.stdout).to_string())
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
