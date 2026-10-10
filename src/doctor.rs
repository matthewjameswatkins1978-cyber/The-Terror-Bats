//! `terrorbats doctor` (W1): honest health checks for the Windows first-class
//! target. Never inspects secrets, never sends telemetry, never fails
//! because optional tools are missing.

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;

use crate::evidence::EvidenceStore;
use crate::supervisor::{CancelToken, ExecutionStatus, SupervisedCommand};

#[derive(Debug, Clone, Serialize)]
pub struct CheckResult {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorReport {
    pub terrorbat_version: String,
    pub core: Vec<CheckResult>,
    pub optional_tools: Vec<CheckResult>,
    pub isolation: Vec<CheckResult>,
    pub overall_ready: bool,
}

fn supervised(program: &str, args: &[&str], timeout: Duration) -> Option<(i32, String)> {
    let mut cmd = SupervisedCommand::new(OsString::from(program));
    cmd.args = args.iter().map(OsString::from).collect();
    cmd.deadline = Some(timeout);
    let out = crate::supervisor::run(&cmd, &CancelToken::new());
    if out.status == ExecutionStatus::Completed {
        Some((
            out.exit_code.unwrap_or(-1),
            out.stdout.as_str_lossy().trim().to_string(),
        ))
    } else {
        None
    }
}

fn check(name: &str, ok: bool, detail: impl Into<String>) -> CheckResult {
    CheckResult {
        name: name.to_string(),
        ok,
        detail: detail.into(),
    }
}

pub fn run_doctor(store_root: Option<PathBuf>) -> DoctorReport {
    let mut core = Vec::new();

    // Platform.
    core.push(check(
        "platform",
        true,
        format!(
            "{} {} (windows version probe: {})",
            std::env::consts::OS,
            std::env::consts::ARCH,
            supervised("cmd", &["/C", "ver"], Duration::from_secs(10))
                .map(|(_, v)| v)
                .unwrap_or_else(|| "unavailable".to_string())
        ),
    ));

    // Git availability + version (also exercises M2 supervision).
    let git = supervised("git", &["--version"], Duration::from_secs(30));
    let git_ok = matches!(&git, Some((0, _)));
    core.push(check(
        "git",
        git_ok,
        git.as_ref()
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| "git not found or did not complete".to_string()),
    ));

    // M2 process supervision probe: a supervised child must complete with
    // captured output and a real exit code.
    core.push(check(
        "process ownership (M2 probe)",
        git_ok,
        if git_ok {
            "supervised `git --version` completed with captured stdout and exit code 0"
        } else {
            "could not supervise a probe process"
        },
    ));

    // Temp directory writable.
    let temp_probe =
        std::env::temp_dir().join(format!("terrorbat-doctor-{}.tmp", std::process::id()));
    let temp_ok =
        std::fs::write(&temp_probe, b"probe").is_ok() && std::fs::remove_file(&temp_probe).is_ok();
    core.push(check(
        "temp directory",
        temp_ok,
        if temp_ok {
            format!("{} writable", std::env::temp_dir().display())
        } else {
            format!("{} NOT writable", std::env::temp_dir().display())
        },
    ));

    // Evidence store: writable + object round-trip verification.
    let store_check = (|| -> Result<(EvidenceStore, String), String> {
        let root = match store_root {
            Some(ref p) => p.clone(),
            None => EvidenceStore::default_root().map_err(|e| e.to_string())?,
        };
        let store = EvidenceStore::open(&root).map_err(|e| e.to_string())?;
        store.self_test().map_err(|e| e.to_string())?;
        Ok((store, root.to_string_lossy().to_string()))
    })();
    let store_ok = store_check.is_ok();
    core.push(check(
        "evidence store",
        store_ok,
        match &store_check {
            Ok((_, root)) => format!("{root} writable; object round-trip verified"),
            Err(e) => e.clone(),
        },
    ));

    // Worktree create + remove support, on a scratch repository in temp.
    let worktree_check = git_ok
        .then(worktree_probe)
        .flatten()
        .unwrap_or_else(|| (false, "skipped: git unavailable".to_string()));
    core.push(check("worktrees", worktree_check.0, worktree_check.1));

    // Optional tools: absence never makes Terror Bats unhealthy.
    let mut optional = Vec::new();
    for (tool, args) in [
        ("cargo", ["--version"].as_slice()),
        ("rustc", ["--version"].as_slice()),
        ("pwsh", ["--version"].as_slice()),
        ("python", ["--version"].as_slice()),
        ("node", ["--version"].as_slice()),
    ] {
        let probed = supervised(tool, args, Duration::from_secs(20));
        optional.push(check(
            tool,
            true, // informational only
            match probed {
                Some((0, v)) => format!("available ({v})"),
                Some((code, v)) => format!("present but exited {code} ({v})"),
                None => "not found (optional)".to_string(),
            },
        ));
    }

    // Isolation honesty: what this build can and cannot offer.
    let isolation = vec![
        check(
            "worktree mode",
            worktree_check.0,
            if worktree_check.0 {
                "AVAILABLE (reversible, observable — NOT hostile-code containment)"
            } else {
                "UNAVAILABLE"
            },
        ),
        check(
            "hostile sandbox",
            true,
            "NOT CONFIGURED (no container/WASI backend in First Flight; host filesystem and network remain UNENFORCED)",
        ),
    ];

    let overall_ready = core.iter().all(|c| c.ok);
    DoctorReport {
        terrorbat_version: crate::runner::TERRORBAT_VERSION.to_string(),
        core,
        optional_tools: optional,
        isolation,
        overall_ready,
    }
}

/// Create a scratch repo, add a worktree, remove it, verify removal.
fn worktree_probe() -> Option<(bool, String)> {
    let dir = std::env::temp_dir().join(format!("terrorbat-doctor-wt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let result = (|| -> Result<(), String> {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let git = |args: &[&str]| -> Result<(), String> {
            let (code, out) = supervised_in(&dir, "git", args, Duration::from_secs(60))
                .ok_or_else(|| format!("git {} failed to complete", args.join(" ")))?;
            if code == 0 {
                Ok(())
            } else {
                Err(format!("git {} exited {code}: {out}", args.join(" ")))
            }
        };
        git(&["init", "-q"])?;
        git(&["config", "user.email", "doctor@terrorbat.local"])?;
        git(&["config", "user.name", "Terror Bat Doctor"])?;
        std::fs::write(dir.join("probe.txt"), "doctor\n").map_err(|e| e.to_string())?;
        git(&["add", "."])?;
        git(&["commit", "-q", "-m", "doctor probe"])?;
        let wt = dir.join("wt");
        git(&["worktree", "add", "--detach", &wt.to_string_lossy(), "HEAD"])?;
        if !wt.exists() {
            return Err("worktree was not created".to_string());
        }
        git(&["worktree", "remove", "--force", &wt.to_string_lossy()])?;
        if wt.exists() {
            return Err("worktree directory survived removal".to_string());
        }
        let (code, listed) =
            supervised_in(&dir, "git", &["worktree", "list"], Duration::from_secs(60))
                .ok_or_else(|| "git worktree list failed to complete".to_string())?;
        if code != 0 {
            return Err(format!("git worktree list exited {code}: {listed}"));
        }
        if listed.lines().count() != 1 {
            return Err("git still registers extra worktrees".to_string());
        }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&dir);
    Some(match result {
        Ok(()) => (
            true,
            "create + remove verified on a scratch repository".to_string(),
        ),
        Err(e) => (false, format!("worktree probe failed: {e}")),
    })
}

fn supervised_in(
    cwd: &std::path::Path,
    program: &str,
    args: &[&str],
    timeout: Duration,
) -> Option<(i32, String)> {
    let mut cmd = SupervisedCommand::new(OsString::from(program));
    cmd.args = args.iter().map(OsString::from).collect();
    cmd.cwd = Some(cwd.to_path_buf());
    cmd.deadline = Some(timeout);
    let out = crate::supervisor::run(&cmd, &CancelToken::new());
    if out.status == ExecutionStatus::Completed {
        let merged = format!("{}{}", out.stdout.as_str_lossy(), out.stderr.as_str_lossy());
        Some((out.exit_code.unwrap_or(-1), merged.trim().to_string()))
    } else {
        None
    }
}
