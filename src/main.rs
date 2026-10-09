//! Terror Bat CLI: spec identity (M1) and disposable-worktree Bat execution
//! (M3/M4). Exit codes are stable and documented; the run manifest/receipt is
//! authoritative, the exit code is a summary.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use terrorbats::ParamOverrides;

#[derive(Parser)]
#[command(
    name = "terrorbats",
    version,
    about = "The Terror Bats Framework — falsification and assurance framework"
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
        /// Explicit local adapter bindings for external Bat steps.
        #[arg(long)]
        adapters: Option<PathBuf>,
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
        /// Explicit external adapter bindings required by external-adapter receipts.
        #[arg(long)]
        adapters: Option<PathBuf>,
        /// Machine-readable comparison output.
        #[arg(long)]
        json: bool,
    },
    /// List universal adapters and their implementation status.
    Adapters {
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },
    /// Inspect one universal adapter's operations and platform limits.
    Adapter {
        #[command(subcommand)]
        command: AdapterCommand,
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
enum AdapterCommand {
    /// Show one adapter's operations, inputs, outputs, constraints,
    /// examples, and platform limitations.
    Inspect {
        /// Adapter name (see `terrorbats adapters`).
        name: String,
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
    /// Execute a Bat Pack serially as a campaign: every entry runs in
    /// declared order per iteration, each child a first-class ordinary run.
    Run {
        /// Path to the Bat Pack YAML.
        pack: PathBuf,
        /// Path to the target repository (must be clean; HEAD is pinned for
        /// the whole campaign).
        #[arg(long)]
        repo: PathBuf,
        /// Explicit local external adapter bindings used by all child runs.
        #[arg(long)]
        adapters: Option<PathBuf>,
        /// Iterations over the pack entries (positive bounded integer).
        #[arg(long, default_value_t = 1)]
        runs: u64,
        /// Stop scheduling further children after the first durably
        /// persisted PROVEN receipt.
        #[arg(long)]
        stop_on_proven: bool,
        /// Evidence store root (default: %LOCALAPPDATA%\TerrorBat).
        #[arg(long)]
        store: Option<PathBuf>,
        /// Machine-readable output (plain campaign schema, never presentation).
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
            eprintln!("terrorbats: {message}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<u8, String> {
    let cli = Cli::parse();
    match cli.command {
        Command::Spec { command } => match command {
            SpecCommand::Check { file } => {
                let human_id = terrorbats::check_spec_file(&file).map_err(|e| e.to_string())?;
                println!("OK  {human_id}");
                Ok(0)
            }
            SpecCommand::Id { file, param } => {
                let overrides = ParamOverrides::parse(&param).map_err(|e| e.to_string())?;
                let identified =
                    terrorbats::identify_spec_file(&file, &overrides).map_err(|e| e.to_string())?;
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
                    terrorbats::identify_spec_file(&file, &overrides).map_err(|e| e.to_string())?;
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
                    terrorbats::pack::identify_pack_file(&pack).map_err(|e| e.to_string())?;
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
                    terrorbats::pack::identify_pack_file(&pack).map_err(|e| e.to_string())?;
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
            PackCommand::Run {
                pack,
                repo,
                runs,
                stop_on_proven,
                store,
                adapters,
                json,
            } => {
                let opts = terrorbats::campaign::CampaignOptions {
                    pack_path: pack,
                    repo,
                    store_root: store,
                    runs,
                    stop_on_proven,
                };
                let out =
                    terrorbats::campaign::run_campaign_with_adapters(&opts, adapters.as_deref())
                        .map_err(|e| e.to_string())?;
                if json {
                    let text =
                        serde_json::to_string_pretty(&out.campaign).map_err(|e| e.to_string())?;
                    println!("{text}");
                } else {
                    let caps = terrorbats::presentation::detect_capabilities();
                    println!(
                        "{}",
                        terrorbats::presentation::render_campaign(&out.campaign, &caps)
                    );
                }
                Ok(terrorbats::campaign::exit_code_for_campaign(&out.campaign))
            }
        },
        Command::Run {
            bat,
            repo,
            adapters,
            param,
            store,
            json,
        } => {
            let bindings = terrorbats::adapter::load_bindings(adapters.as_deref())
                .map_err(|e| e.to_string())?;
            let opts = terrorbats::runner::RunOptions {
                bat_path: bat,
                repo,
                store_root: store,
                overrides: param,
            };
            let out = terrorbats::runner::run_bat_with_adapters(&opts, bindings.as_ref())
                .map_err(|e| e.to_string())?;
            if json {
                let text = serde_json::to_string_pretty(&out.receipt).map_err(|e| e.to_string())?;
                println!("{text}");
            } else {
                let caps = terrorbats::presentation::detect_capabilities();
                println!(
                    "{}",
                    terrorbats::presentation::render_receipt(&out.receipt, &caps)
                );
            }
            Ok(terrorbats::runner::exit_code_for(
                out.manifest.run_status,
                Some(out.receipt.verdict.as_str()),
            ))
        }
        Command::Inspect { id, store, json } => {
            if id.starts_with("campaign:sha256:") {
                return inspect_campaign(&id, store, json);
            }
            let store = open_store(store)?;
            let run_dir = terrorbats::receipt::locate_run(&store, &id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| {
                    format!(
                        "no run or receipt with id `{id}` in store `{}`",
                        store.root().display()
                    )
                })?;
            let receipt = terrorbats::receipt::load_receipt(&run_dir).map_err(|e| e.to_string())?;
            if json {
                let text = serde_json::to_string_pretty(&receipt).map_err(|e| e.to_string())?;
                println!("{text}");
            } else {
                let caps = terrorbats::presentation::detect_capabilities();
                println!(
                    "{}",
                    terrorbats::presentation::render_receipt(&receipt, &caps)
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
                let eref = terrorbats::evidence::EvidenceRef(reference.clone());
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
        Command::Replay {
            id,
            store,
            adapters,
            json,
        } => {
            let (report, out) =
                terrorbats::receipt::replay_with_adapters(&id, store, adapters.as_deref())
                    .map_err(|e| e.to_string())?;
            if json {
                let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
                println!("{text}");
            } else {
                let caps = terrorbats::presentation::detect_capabilities();
                println!(
                    "{}",
                    terrorbats::presentation::render_replay(&report, &out.receipt, &caps)
                );
            }
            Ok(terrorbats::runner::exit_code_for(
                out.manifest.run_status,
                Some(out.receipt.verdict.as_str()),
            ))
        }
        Command::Adapters { json } => {
            let adapters = terrorbats::adapters::all();
            if json {
                let text = serde_json::to_string_pretty(&adapters).map_err(|e| e.to_string())?;
                println!("{text}");
            } else {
                let caps = terrorbats::presentation::detect_capabilities();
                println!(
                    "{}",
                    terrorbats::presentation::render_adapters(&adapters, &caps)
                );
            }
            Ok(0)
        }
        Command::Adapter { command } => match command {
            AdapterCommand::Inspect { name, json } => {
                let info = terrorbats::adapters::find(&name).ok_or_else(|| {
                    format!("unknown adapter `{name}` (see `terrorbats adapters`)")
                })?;
                if json {
                    let text = serde_json::to_string_pretty(&info).map_err(|e| e.to_string())?;
                    println!("{text}");
                } else {
                    let caps = terrorbats::presentation::detect_capabilities();
                    println!(
                        "{}",
                        terrorbats::presentation::render_adapter_inspect(&info, &caps)
                    );
                }
                Ok(0)
            }
        },
        Command::Doctor { store, json } => {
            let report = terrorbats::doctor::run_doctor(store);
            if json {
                let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
                println!("{text}");
            } else {
                let caps = terrorbats::presentation::detect_capabilities();
                println!(
                    "{}",
                    terrorbats::presentation::render_doctor(&report, &caps)
                );
            }
            Ok(if report.overall_ready { 0 } else { 3 })
        }
    }
}

fn inspect_campaign(id: &str, store: Option<PathBuf>, json: bool) -> Result<u8, String> {
    terrorbats::campaign::validate_campaign_id(id)?;
    let store = open_store(store)?;
    let campaign_dir = terrorbats::campaign::locate_campaign(&store, id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| {
            format!(
                "no campaign with id `{id}` in store `{}`",
                store.root().display()
            )
        })?;
    // Trusted load: a corrupt campaign, corrupt child, or missing child
    // refuses the aggregate here — success is never rendered over corruption.
    let campaign =
        terrorbats::campaign::load_campaign(&store, &campaign_dir).map_err(|e| e.to_string())?;
    if json {
        let text = serde_json::to_string_pretty(&campaign).map_err(|e| e.to_string())?;
        println!("{text}");
    } else {
        let caps = terrorbats::presentation::detect_capabilities();
        println!(
            "{}",
            terrorbats::presentation::render_campaign(&campaign, &caps)
        );
    }
    Ok(0)
}

fn open_store(store: Option<PathBuf>) -> Result<terrorbats::evidence::EvidenceStore, String> {
    let root = match store {
        Some(p) => p,
        None => terrorbats::evidence::EvidenceStore::default_root().map_err(|e| e.to_string())?,
    };
    terrorbats::evidence::EvidenceStore::open(&root).map_err(|e| e.to_string())
}

// Human rendering lives in terrorbats::presentation: one receipt model
// projected to a Sartorial Document. The CLI owns no renderers, and the
// machine (--json) path never touches presentation.
