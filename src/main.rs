//! Tiny CLI exposing M1 functionality only. No Bats are run.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use terrorbat::ParamOverrides;

#[derive(Parser)]
#[command(
    name = "terrorbat",
    version,
    about = "Terror Bat — falsification and assurance framework (M1: Bat Spec identity)"
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

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("terrorbat: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let cli = Cli::parse();
    let Command::Spec { command } = cli.command;
    match command {
        SpecCommand::Check { file } => {
            let human_id = terrorbat::check_spec_file(&file).map_err(|e| e.to_string())?;
            println!("OK  {human_id}");
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
        }
        SpecCommand::Canonical { file, param } => {
            let overrides = ParamOverrides::parse(&param).map_err(|e| e.to_string())?;
            let identified =
                terrorbat::identify_spec_file(&file, &overrides).map_err(|e| e.to_string())?;
            println!("{}", identified.canonical_json);
        }
    }
    Ok(())
}
