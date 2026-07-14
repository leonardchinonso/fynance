//! Shared state injected into every Axum handler.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::importers::provider::ProgressTx;
use crate::storage::Db;

/// `Mutex::lock` that recovers the guard when a previous panic poisoned the
/// mutex. One panicked handler must not take the whole server down; SQLite
/// writes are transactional, so the connection stays consistent.
pub fn lock_or_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Clone-cheap bundle of process-wide resources.
///
/// `rusqlite::Connection` is `Send` but not `Sync`, so we guard `Db`
/// behind a `Mutex`. At single-user local scale, holding the mutex
/// across a single query is cheap and means handlers don't have to
/// coordinate explicitly. A connection pool (r2d2) would be a drop-in
/// replacement when we outgrow this.
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<Db>>,
    /// Active parse progress channels, keyed by client-supplied `parse_id`.
    pub progress_channels: Arc<Mutex<HashMap<String, ProgressTx>>>,
    /// True when the Axum listener is bound to a loopback address.
    ///
    /// When this is the case, the OS network boundary already
    /// guarantees every request comes from the same user on the same
    /// machine, so the browser UI is trusted and `/api/*` routes do
    /// not require a bearer token. When the binary runs in Docker or
    /// another non-loopback environment, this is `false` and the auth
    /// middleware requires a token for every non-public API route.
    pub loopback_only: bool,
}

impl AppState {
    pub fn db(&self) -> MutexGuard<'_, Db> {
        lock_or_recover(&self.db)
    }

    pub fn progress_channels(&self) -> MutexGuard<'_, HashMap<String, ProgressTx>> {
        lock_or_recover(&self.progress_channels)
    }
}
