//! Terror Bat CLI: spec identity (M1) and disposable-worktree Bat execution
//! (M3/M4). Exit codes are stable and documented; the run manifest/receipt is
//! authoritative, the exit code is a summary.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use terrorbat::ParamOverrides;

#[derive(Parser)]
#[command(
    name = "terrorbat",
    version,
    about = "Terror Bat — falsification and assurance framework"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Bat Spec operations (parse, resolve, canonicalise, identify).
    Spec {
        #[command(subcommand)]
        command: SpecCommand,
    },
    /// Execute a Bat against a target repository in a disposable worktree.
    Run {
        /// Path to the Bat Spec YAML.
        bat: PathBuf,
        /// Path to the target repository (must be clean).
        #[arg(long)]
        repo: PathBuf,
        /// Parameter override as `name=value` (repeatable).
        #[arg(long = "param", value_name = "NAME=VALUE")]
        param: Vec<String>,
        /// Evidence store root (default: %LOCALAPPDATA%\TerrorBat).
        #[arg(long)]
        store: Option<PathBuf>,
        /// Machine-readable manifest output.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum SpecCommand {
    /// Validate a Bat Spec file.
    Check {
        /// Path to the YAML Bat Spec.
        file: PathBuf,
    },
    /// Print the content identities of a resolved Bat Spec.
    Id {
        /// Path to the YAML Bat Spec.
        file: PathBuf,
        /// Parameter override as `name=value` (repeatable).
        #[arg(long = "param", value_name = "NAME=VALUE")]
        param: Vec<String>,
    },
    /// Print the exact canonical semantic JSON used for the Bat hash.
    Canonical {
        /// Path to the YAML Bat Spec.
        file: PathBuf,
        /// Parameter override as `name=value` (repeatable).
        #[arg(long = "param", value_name = "NAME=VALUE")]
        param: Vec<String>,
    },
}

/// Stable CLI exit codes (documented in docs/first-flight.md):
/// 0 = framework operation completed, no proven falsification
/// 1 = claim falsified / PROVEN finding
/// 2 = invalid request / spec / policy issue
/// 3 = infrastructure failure / inconclusive execution
fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            eprintln!("terrorbat: {message}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<u8, String> {
    let cli = Cli::parse();
    match cli.command {
        Command::Spec { command } => match command {
            SpecCommand::Check { file } => {
                let human_id = terrorbat::check_spec_file(&file).map_err(|e| e.to_string())?;
                println!("OK  {human_id}");
                Ok(0)
            }
            SpecCommand::Id { file, param } => {
                let overrides = ParamOverrides::parse(&param).map_err(|e| e.to_string())?;
                let identified =
                    terrorbat::identify_spec_file(&file, &overrides).map_err(|e| e.to_string())?;
                let ids = identified.identities;
                println!("bat     {}", ids.bat);
                println!("claim   {}", ids.claim);
                println!("attack  {}", ids.attack);
                println!("oracle  {}", ids.oracle);
                Ok(0)
            }
            SpecCommand::Canonical { file, param } => {
                let overrides = ParamOverrides::parse(&param).map_err(|e| e.to_string())?;
                let identified =
                    terrorbat::identify_spec_file(&file, &overrides).map_err(|e| e.to_string())?;
                println!("{}", identified.canonical_json);
                Ok(0)
            }
        },
        Command::Run {
            bat,
            repo,
            param,
            store,
            json,
        } => {
            let opts = terrorbat::runner::RunOptions {
                bat_path: bat,
                repo,
                store_root: store,
                overrides: param,
            };
            let out = terrorbat::runner::run_bat(&opts).map_err(|e| e.to_string())?;
            if json {
                let text =
                    serde_json::to_string_pretty(&out.manifest).map_err(|e| e.to_string())?;
                println!("{text}");
            } else {
                render_run_summary(&out.manifest);
            }
            Ok(terrorbat::runner::exit_code_for(
                out.manifest.run_status,
                None,
            ))
        }
    }
}

fn render_run_summary(m: &terrorbat::runner::RunManifest) {
    use terrorbat::runner::{CleanupStatus, RunStatus};
    println!("🦇 {}", m.bat.id);
    println!();
    println!("Target");
    println!("  {}", m.target.root);
    println!("  {}", short_sha(&m.target.commit));
    println!();
    println!("Attack");
    for phase in ["setup", "run"] {
        let total = m.steps.iter().filter(|s| s.phase == phase).count();
        let done = m
            .steps
            .iter()
            .filter(|s| s.phase == phase && s.status == RunStatus::Completed)
            .count();
        println!("  {phase:<6} {done}/{total} completed");
    }
    println!();
    println!("Execution");
    println!("  {:?}", m.run_status);
    if let Some(bad) = m.steps.iter().find(|s| s.status != RunStatus::Completed) {
        println!(
            "  halted at {}[{}] {}.{}",
            bad.phase, bad.index, bad.adapter, bad.action
        );
        if let Some(err) = &bad.error {
            println!("  {err}");
        }
    }
    println!();
    println!("Evidence");
    if let Some(d) = &m.captures.git_diff {
        println!("  git diff    {d}");
    }
    if let Some(s) = &m.captures.git_status {
        println!("  git status  {s}");
    }
    println!("  run dir     {}", m.run_dir);
    println!();
    println!("Cleanup");
    match m.worktree.cleanup.status {
        CleanupStatus::Succeeded => println!("  succeeded"),
        CleanupStatus::NotAttempted => println!("  not attempted"),
        CleanupStatus::Failed => {
            println!("  FAILED");
            if let Some(err) = &m.worktree.cleanup.error {
                for line in err.lines() {
                    println!("  {line}");
                }
            }
        }
    }
}

fn short_sha(sha: &str) -> String {
    if sha.len() > 12 {
        sha[..12].to_string()
    } else {
        sha.to_string()
    }
}
