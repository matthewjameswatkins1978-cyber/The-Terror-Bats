//! Disposable Git worktree lifecycle (M3).
//!
//! ```text
//! source repository → validate clean → pin exact commit
//!   → git worktree add --detach <path> <sha>
//!   → run → inspect → git worktree remove --force
//! ```
//!
//! Native Git CLI through the M2 supervisor; Git is not reimplemented.
//!
//! Isolation honesty: a disposable worktree provides reversibility,
//! source-tree protection, mutation observation, and a repeatable starting
//! state. It is NOT hostile-code containment: no host filesystem sandboxing,
//! no network isolation, no credential isolation.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

use crate::error::{Error, Result};
use crate::supervisor::{CancelToken, ExecutionStatus, SupervisedCommand, SupervisedOutcome};

/// Bound for any single Git invocation made by Terror Bats itself.
const GIT_DEADLINE: Duration = Duration::from_secs(120);

/// Run a Git command and require a zero exit.
pub fn git(repo: &Path, args: &[&str]) -> Result<SupervisedOutcome> {
    git_with(repo, args, None)
}

/// Run a Git command with an optional working-directory override.
pub fn git_with(repo: &Path, args: &[&str], cwd: Option<&Path>) -> Result<SupervisedOutcome> {
    let mut cmd = SupervisedCommand::new(OsString::from("git"));
    cmd.args = args.iter().map(OsString::from).collect();
    cmd.cwd = Some(cwd.unwrap_or(repo).to_path_buf());
    cmd.deadline = Some(GIT_DEADLINE);
    let out = crate::supervisor::run(&cmd, &CancelToken::new());
    match out.status {
        ExecutionStatus::Completed => Ok(out),
        ExecutionStatus::TimedOut => Err(Error::spec(
            repo,
            format!("git {} timed out after {GIT_DEADLINE:?}", args.join(" ")),
        )),
        other => Err(Error::spec(
            repo,
            format!(
                "git {} failed mechanically ({other:?}): {}",
                args.join(" "),
                out.error.clone().unwrap_or_default()
            ),
        )),
    }
}

fn git_ok(repo: &Path, args: &[&str], what: &str) -> Result<String> {
    let out = git(repo, args)?;
    if out.exit_code != Some(0) {
        return Err(Error::spec(
            repo,
            format!(
                "{what}: `git {}` exited with {:?}\nstderr: {}",
                args.join(" "),
                out.exit_code,
                out.stderr.as_str_lossy().trim()
            ),
        ));
    }
    Ok(out.stdout.as_str_lossy().trim().to_string())
}

/// What the target repository looked like when Terror Bats accepted it.
#[derive(Debug, Clone, Serialize)]
pub struct TargetState {
    pub root: PathBuf,
    pub commit: String,
    pub branch: Option<String>,
    pub dirty: bool,
    /// `git status --porcelain` output at acceptance time (empty when clean).
    pub status_porcelain: String,
}

/// Inspect and validate the target repository. Refuses dirty repositories:
/// Terror Bats will not silently exclude or incorporate local changes.
pub fn inspect_target(repo: &Path) -> Result<TargetState> {
    if !repo.exists() {
        return Err(Error::spec(repo, "target repository path does not exist"));
    }
    let root_str = git_ok(
        repo,
        &["rev-parse", "--show-toplevel"],
        "not inside a Git repository",
    )?;
    let root = PathBuf::from(&root_str);
    let commit = git_ok(&root, &["rev-parse", "HEAD"], "cannot resolve HEAD commit")?;
    // Detached HEAD yields an empty branch name; that is a valid pinned state.
    let branch_out = git(&root, &["branch", "--show-current"])?;
    let branch = if branch_out.exit_code == Some(0) {
        let name = branch_out.stdout.as_str_lossy().trim().to_string();
        if name.is_empty() { None } else { Some(name) }
    } else {
        None
    };
    let status = git_ok(
        &root,
        &["status", "--porcelain"],
        "cannot read working-tree status",
    )?;
    let dirty = !status.trim().is_empty();
    Ok(TargetState {
        root,
        commit,
        branch,
        dirty,
        status_porcelain: status,
    })
}

/// Create a detached disposable worktree at `path` pinned to `commit`.
pub fn create_worktree(repo: &Path, commit: &str, path: &Path) -> Result<()> {
    if path.exists() {
        return Err(Error::spec(
            path,
            "worktree location already exists; refusing to reuse",
        ));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Error::spec(parent, format!("cannot create worktree parent: {e}")))?;
    }
    let out = git(
        repo,
        &[
            "worktree",
            "add",
            "--detach",
            &path.to_string_lossy(),
            commit,
        ],
    )?;
    if out.exit_code != Some(0) {
        return Err(Error::spec(
            repo,
            format!(
                "cannot create disposable worktree at `{}` from commit {commit}:\n{}",
                path.display(),
                out.stderr.as_str_lossy().trim()
            ),
        ));
    }
    Ok(())
}

/// Remove a worktree with bounded retry/backoff (Windows scanners and file
/// handles cause transient failures), then validate Git no longer lists it.
/// Never retries forever; reports truthfully on exhaustion.
pub fn remove_worktree(repo: &Path, path: &Path) -> std::result::Result<(), String> {
    let attempts = 3;
    let mut last_err = String::new();
    for attempt in 1..=attempts {
        let out = git(
            repo,
            &["worktree", "remove", "--force", &path.to_string_lossy()],
        )
        .map_err(|e| e.to_string())?;
        if out.exit_code == Some(0) {
            // Validate: Git must no longer register the worktree.
            let listed = git(repo, &["worktree", "list", "--porcelain"])
                .map_err(|e| e.to_string())?
                .stdout
                .as_str_lossy()
                .to_string();
            let target = path.to_string_lossy().replace('\\', "/");
            if listed.lines().any(|l| {
                let l = l.replace('\\', "/");
                l.starts_with("worktree ") && l.trim_start_matches("worktree ") == target
            }) {
                last_err = "git still lists the worktree after removal".to_string();
            } else {
                return Ok(());
            }
        } else {
            last_err = out.stderr.as_str_lossy().trim().to_string();
        }
        if attempt < attempts {
            std::thread::sleep(Duration::from_millis(400 * attempt));
        }
    }
    Err(format!(
        "worktree removal failed after {attempts} attempts: {last_err}"
    ))
}

/// Where disposable worktrees live: `%TEMP%\terrorbat\<execution-id>\worktree`.
/// Outside the target source tree, short root, clearly owned.
pub fn worktree_path(execution_id: &str) -> Result<PathBuf> {
    let temp = std::env::temp_dir();
    Ok(temp.join("terrorbat").join(execution_id).join("worktree"))
}
