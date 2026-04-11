//! Clap definitions for the `fynance` CLI.
//!
//! Phase 1 exposes `import`, `stats`, `account`, and `budget`. Later
//! phases will add `serve`, `token`, `monthly`, and `export` without
//! touching the Phase 1 commands.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "fynance",
    version,
    about = "Local-first personal finance tracker"
)]
pub struct Cli {
    /// Override the default database path. Takes precedence over the
    /// `FYNANCE_DB_PATH` env var.
    #[arg(long, global = true)]
    pub db: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Import a CSV file or a directory of CSVs into the database.
    Import {
        /// File or directory path.
        path: PathBuf,
        /// Account id that these transactions belong to.
        #[arg(long)]
        account: String,
    },
    /// Print a summary of what's in the database.
    Stats,
    /// Manage accounts.
    Account {
        #[command(subcommand)]
        command: AccountCommand,
    },
    /// Manage monthly budgets.
    Budget {
        #[command(subcommand)]
        command: BudgetCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum AccountCommand {
    /// Register a new account or update an existing one.
    Add {
        #[arg(long)]
        id: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        institution: String,
        #[arg(long = "type")]
        account_type: String,
        #[arg(long)]
        currency: Option<String>,
        #[arg(long)]
        balance: Option<String>,
    },
    /// Record a new balance snapshot for an existing account.
    SetBalance {
        id: String,
        amount: String,
        #[arg(long)]
        date: String,
    },
    /// Print all registered accounts.
    List,
}

#[derive(Debug, Subcommand)]
pub enum BudgetCommand {
    /// Set a category's monthly budget target.
    Set {
        #[arg(long)]
        month: String,
        #[arg(long)]
        category: String,
        #[arg(long)]
        amount: String,
    },
    /// Print budgets for a given month.
    Status {
        #[arg(long)]
        month: String,
    },
}
