use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use tracing_subscriber::EnvFilter;

use fynance::cli::{AccountCommand, BudgetCommand, Cli, Commands};
use fynance::commands::{account, budget, import, stats};
use fynance::storage::Db;
use fynance::storage::db::default_db_path;

fn main() -> Result<()> {
    // Load .env files from the current working directory so config env
    // vars (FYNANCE_DB_PATH, FYNANCE_LOG_LEVEL) are available without a
    // wrapping shell. Ignore errors: running without a .env file is fine.
    let _ = dotenvy::dotenv();

    // tracing_subscriber reads RUST_LOG first; fall back to FYNANCE_LOG_LEVEL
    // so users can set either.
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| {
            let level = std::env::var("FYNANCE_LOG_LEVEL").unwrap_or_else(|_| "info".to_string());
            EnvFilter::try_new(level)
        })
        .unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let cli = Cli::parse();

    let db_path = resolve_db_path(cli.db.as_deref())?;
    let db = Db::open(&db_path)?;

    match cli.command {
        Commands::Import { path, account } => import::run(&db, &path, &account),
        Commands::Stats => stats::run(&db),
        Commands::Account { command } => match command {
            AccountCommand::Add {
                id,
                name,
                institution,
                account_type,
                currency,
                balance,
            } => account::add(
                &db,
                &id,
                &name,
                &institution,
                &account_type,
                currency.as_deref(),
                balance.as_deref(),
            ),
            AccountCommand::SetBalance { id, amount, date } => {
                account::set_balance(&db, &id, &amount, &date)
            }
            AccountCommand::List => account::list(&db),
        },
        Commands::Budget { command } => match command {
            BudgetCommand::Set {
                month,
                category,
                amount,
            } => budget::set(&db, &month, &category, &amount),
            BudgetCommand::Status { month } => budget::status(&db, &month),
        },
    }
}

/// Decide which SQLite file to open. Precedence:
/// 1. `--db` CLI flag (explicit wins)
/// 2. `FYNANCE_DB_PATH` env var
/// 3. OS-native default (`dirs::data_local_dir()`)
fn resolve_db_path(cli: Option<&std::path::Path>) -> Result<PathBuf> {
    if let Some(p) = cli {
        return Ok(p.to_path_buf());
    }
    if let Ok(env) = std::env::var("FYNANCE_DB_PATH") {
        if !env.is_empty() {
            return Ok(PathBuf::from(env));
        }
    }
    default_db_path()
}
