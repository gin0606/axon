use axon_lifecycle_migration::{Result, job, util};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    version,
    about = "Prepare, verify and explicitly apply a legacy Axon lifecycle migration",
    after_help = "Jobs retain backups and immutable candidates. Apply and restore require all writers and SQLite connections to be stopped. No Git, PATH or plugin changes are made."
)]
struct Cli {
    #[command(subcommand)]
    command: Operation,
}
#[derive(Subcommand)]
enum Operation {
    /// Back up explicit roots and verify candidates without replacing live data
    Prepare {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        job: PathBuf,
    },
    /// Apply a fully prepared job, stopping at the first failed target
    Apply {
        #[arg(long)]
        job: PathBuf,
        #[arg(long)]
        writers_stopped: bool,
    },
    /// Restore backups only where no post-migration writes would be lost
    Restore {
        #[arg(long)]
        job: PathBuf,
        #[arg(long)]
        writers_stopped: bool,
    },
}
fn run(cli: Cli) -> Result<()> {
    let result = match cli.command {
        Operation::Prepare { config, job } => job::prepare(&util::load(&config)?, &job)?,
        Operation::Apply {
            job,
            writers_stopped,
        } => job::execute(&job, false, writers_stopped)?,
        Operation::Restore {
            job,
            writers_stopped,
        } => job::execute(&job, true, writers_stopped)?,
    };
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
fn main() {
    if let Err(error) = run(Cli::parse()) {
        eprintln!("Error: {}", util::human(error));
        std::process::exit(1);
    }
}
