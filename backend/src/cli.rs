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
    /// Manage profiles (logical owners that accounts belong to).
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },
    /// Manage monthly budgets.
    Budget {
        #[command(subcommand)]
        command: BudgetCommand,
    },
    /// Start the local Axum web server.
    Serve {
        /// Override the listen port. Defaults to `FYNANCE_PORT` or 7433.
        #[arg(long)]
        port: Option<u16>,
        /// Skip the automatic browser launch.
        #[arg(long = "no-open")]
        no_open: bool,
    },
    /// Manage API bearer tokens used by scripts and external agents.
    Token {
        #[command(subcommand)]
        command: TokenCommand,
    },
    /// Manage transactions.
    Transaction {
        #[command(subcommand)]
        command: TransactionCommand,
    },
    /// One-time data migration: convert rows stored in a broker sub-unit
    /// currency (GBX, USX, ZAC, ILA) to their parent currency. Defaults to a
    /// dry run that prints what would change without writing anything.
    ///
    /// Run this BEFORE deleting a sub-unit currency by hand: conversion of a
    /// currency with no stored rate silently returns the amount unchanged.
    /// Idempotent and re-runnable after a partial failure, but not atomic
    /// across tables.
    MigrateSubunits {
        /// Actually write the changes. Without this flag, prints a report
        /// and writes nothing.
        #[arg(long)]
        apply: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum TransactionCommand {
    /// Permanently delete transactions. Pass one or more ids, or use
    /// `--account <id>` to delete every transaction for an account (e.g. to
    /// clear it before deleting the account). This is a hard delete.
    Delete {
        /// Transaction id(s) to delete.
        ids: Vec<String>,
        /// Delete every transaction belonging to this account id instead.
        #[arg(long)]
        account: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum TokenCommand {
    /// Generate a new bearer token. The raw value is printed once.
    Create {
        #[arg(long)]
        name: String,
    },
    /// List all known tokens with their active/revoked status.
    List,
    /// Revoke (deactivate) a token by name.
    Revoke {
        #[arg(long)]
        name: String,
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
        /// Profile(s) this account belongs to. Required; repeat for multiple.
        /// The profile must already exist (create one via the web UI/API).
        #[arg(long = "profile", required = true)]
        profiles: Vec<String>,
    },
    /// Record a new balance snapshot for an existing account (writes a `_CASH`
    /// holding; the API exposes the aggregated balance derived from holdings).
    SetBalance {
        id: String,
        amount: String,
        #[arg(long)]
        date: String,
    },
    /// Print all registered accounts.
    List,
    /// Delete an account. Refuses if it still has transactions or holdings.
    /// Defaults to a soft delete (deactivate); pass `--hard` to permanently
    /// remove the row.
    Delete {
        /// Account id to delete.
        id: String,
        /// Permanently remove the row instead of deactivating it.
        #[arg(long)]
        hard: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum ProfileCommand {
    /// Create a new profile. Accounts must reference an existing profile.
    Add {
        #[arg(long)]
        id: String,
        #[arg(long)]
        name: String,
    },
    /// Print all profiles.
    List,
    /// Delete a profile. Refuses if any account still references it.
    Delete {
        /// Profile id to delete.
        id: String,
    },
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
