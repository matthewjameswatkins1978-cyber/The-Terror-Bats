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
    /// Bat Pack operations (validate manifests, identify packs).
    Pack {
        #[command(subcommand)]
        command: PackCommand,
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
        /// Machine-readable receipt output.
        #[arg(long)]
        json: bool,
    },
    /// Inspect a run or receipt by execution id or receipt id.
    Inspect {
        /// Execution id or `receipt:sha256:...`.
        id: String,
        /// Evidence store root (default: %LOCALAPPDATA%\TerrorBat).
        #[arg(long)]
        store: Option<PathBuf>,
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },
    /// Evidence object operations.
    Evidence {
        #[command(subcommand)]
        command: EvidenceCommand,
    },
    /// Replay a receipt as a new execution (the original stays immutable).
    Replay {
        /// Execution id or `receipt:sha256:...`.
        id: String,
        /// Evidence store root (default: %LOCALAPPDATA%\TerrorBat).
        #[arg(long)]
        store: Option<PathBuf>,
        /// Machine-readable comparison output.
        #[arg(long)]
        json: bool,
    },
    /// Check platform readiness (Git, store, worktrees, supervision).
    Doctor {
        /// Evidence store root (default: %LOCALAPPDATA%\TerrorBat).
        #[arg(long)]
        store: Option<PathBuf>,
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum PackCommand {
    /// Validate a Bat Pack file and all of its effective Bats.
    Check {
        /// Path to the Bat Pack YAML.
        pack: PathBuf,
        /// Evidence store root (accepted for surface consistency; pack
        /// validation performs no store reads or writes).
        #[arg(long)]
        store: Option<PathBuf>,
        /// Machine-readable output (plain schema, never presentation).
        #[arg(long)]
        json: bool,
    },
    /// Print the content identity of a Bat Pack and its entries.
    Id {
        /// Path to the Bat Pack YAML.
        pack: PathBuf,
        /// Evidence store root (accepted for surface consistency; pack
        /// identification performs no store reads or writes).
        #[arg(long)]
        store: Option<PathBuf>,
        /// Machine-readable output (plain schema, never presentation).
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum EvidenceCommand {
    /// Show an evidence object (digest verified on read).
    Show {
        /// `evidence:sha256:...` reference.
        reference: String,
        /// Evidence store root (default: %LOCALAPPDATA%\TerrorBat).
        #[arg(long)]
        store: Option<PathBuf>,
        /// Write raw bytes to this file instead of printing text.
        #[arg(long)]
        out: Option<PathBuf>,
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
        Command::Pack { command } => match command {
            PackCommand::Check { pack, store, json } => {
                if let Some(root) = store {
                    open_store(Some(root))?;
                }
                let identified =
                    terrorbat::pack::identify_pack_file(&pack).map_err(|e| e.to_string())?;
                if json {
                    let text = serde_json::to_string_pretty(&identified.to_json())
                        .map_err(|e| e.to_string())?;
                    println!("{text}");
                } else {
                    println!("OK  {}", identified.human_id);
                }
                Ok(0)
            }
            PackCommand::Id { pack, store, json } => {
                if let Some(root) = store {
                    open_store(Some(root))?;
                }
                let identified =
                    terrorbat::pack::identify_pack_file(&pack).map_err(|e| e.to_string())?;
                if json {
                    let text = serde_json::to_string_pretty(&identified.to_json())
                        .map_err(|e| e.to_string())?;
                    println!("{text}");
                } else {
                    println!("pack  {}", identified.identity);
                    for entry in &identified.entries {
                        println!("bat   {}  {}", entry.bat, entry.path);
                    }
                }
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
                let text = serde_json::to_string_pretty(&out.receipt).map_err(|e| e.to_string())?;
                println!("{text}");
            } else {
                let caps = terrorbat::presentation::detect_capabilities();
                println!(
                    "{}",
                    terrorbat::presentation::render_receipt(&out.receipt, &caps)
                );
            }
            Ok(terrorbat::runner::exit_code_for(
                out.manifest.run_status,
                Some(out.receipt.verdict.as_str()),
            ))
        }
        Command::Inspect { id, store, json } => {
            let store = open_store(store)?;
            let run_dir = terrorbat::receipt::locate_run(&store, &id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| {
                    format!(
                        "no run or receipt with id `{id}` in store `{}`",
                        store.root().display()
                    )
                })?;
            let receipt = terrorbat::receipt::load_receipt(&run_dir).map_err(|e| e.to_string())?;
            if json {
                let text = serde_json::to_string_pretty(&receipt).map_err(|e| e.to_string())?;
                println!("{text}");
            } else {
                let caps = terrorbat::presentation::detect_capabilities();
                println!(
                    "{}",
                    terrorbat::presentation::render_receipt(&receipt, &caps)
                );
            }
            Ok(0)
        }
        Command::Evidence { command } => match command {
            EvidenceCommand::Show {
                reference,
                store,
                out,
            } => {
                let store = open_store(store)?;
                let eref = terrorbat::evidence::EvidenceRef(reference.clone());
                let bytes = store.get(&eref).map_err(|e| e.to_string())?;
                if let Some(path) = out {
                    std::fs::write(&path, &bytes)
                        .map_err(|e| format!("cannot write `{}`: {e}", path.display()))?;
                    println!("wrote {} bytes to {}", bytes.len(), path.display());
                } else if let Ok(text) = std::str::from_utf8(&bytes) {
                    print!("{text}");
                    if !text.ends_with('\n') {
                        println!();
                    }
                } else {
                    println!("binary evidence (not dumped to the terminal)");
                    println!("  reference  {reference}");
                    println!("  bytes      {}", bytes.len());
                    println!("  use --out <file> to extract");
                }
                Ok(0)
            }
        },
        Command::Replay { id, store, json } => {
            let (report, out) =
                terrorbat::receipt::replay(&id, store).map_err(|e| e.to_string())?;
            if json {
                let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
                println!("{text}");
            } else {
                let caps = terrorbat::presentation::detect_capabilities();
                println!(
                    "{}",
                    terrorbat::presentation::render_replay(&report, &out.receipt, &caps)
                );
            }
            Ok(terrorbat::runner::exit_code_for(
                out.manifest.run_status,
                Some(out.receipt.verdict.as_str()),
            ))
        }
        Command::Doctor { store, json } => {
            let report = terrorbat::doctor::run_doctor(store);
            if json {
                let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
                println!("{text}");
            } else {
                let caps = terrorbat::presentation::detect_capabilities();
                println!("{}", terrorbat::presentation::render_doctor(&report, &caps));
            }
            Ok(if report.overall_ready { 0 } else { 3 })
        }
    }
}

fn open_store(store: Option<PathBuf>) -> Result<terrorbat::evidence::EvidenceStore, String> {
    let root = match store {
        Some(p) => p,
        None => terrorbat::evidence::EvidenceStore::default_root().map_err(|e| e.to_string())?,
    };
    terrorbat::evidence::EvidenceStore::open(&root).map_err(|e| e.to_string())
}

// Human rendering lives in terrorbat::presentation: one receipt model
// projected to a Sartorial Document. The CLI owns no renderers, and the
// machine (--json) path never touches presentation.
