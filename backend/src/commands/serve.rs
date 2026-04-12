//! `fynance serve` — start the local Axum HTTP server.
//!
//! This is the primary entrypoint for the app: the user runs one
//! command, the binary opens the default browser, and all further
//! interaction happens over HTTP on loopback. Phase 2 establishes the
//! server scaffold; later phases plug real endpoints into the router.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use tokio::net::TcpListener;

use crate::server;
use crate::storage::Db;

const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 7433;

pub fn run(db: Db, no_open: bool, port_override: Option<u16>) -> Result<()> {
    // `tokio::runtime::Runtime::new()` rather than `#[tokio::main]` so
    // the rest of the CLI stays synchronous. Only `serve` needs async.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("building tokio runtime")?;
    runtime.block_on(async move {
        let host = std::env::var("FYNANCE_HOST").unwrap_or_else(|_| DEFAULT_HOST.to_string());
        let port = port_override
            .or_else(|| {
                std::env::var("FYNANCE_PORT")
                    .ok()
                    .and_then(|s| s.parse().ok())
            })
            .unwrap_or(DEFAULT_PORT);

        let addr: SocketAddr = format!("{host}:{port}")
            .parse()
            .with_context(|| format!("parsing {host}:{port} as a socket address"))?;

        let loopback_only = addr.ip().is_loopback();
        let app = server::build_router(Arc::new(Mutex::new(db)), loopback_only);

        let listener = TcpListener::bind(addr)
            .await
            .with_context(|| format!("binding tcp listener on {addr}"))?;

        // Use the user-facing `localhost` in the printed URL even when
        // the listener bound to 127.0.0.1 so copy/paste into a browser
        // works across operating systems.
        let display_host = if host == "0.0.0.0" {
            "localhost"
        } else {
            host.as_str()
        };
        let url = format!("http://{display_host}:{port}");

        tracing::info!(%addr, "fynance: server started at {url}");
        println!("fynance: server started at {url}");

        if !no_open {
            // `open::that` can fail in headless environments; that's
            // fine, the user can still navigate manually.
            if let Err(err) = open::that(&url) {
                tracing::warn!(?err, "failed to auto-open browser");
            }
        }

        axum::serve(listener, app.into_make_service())
            .await
            .context("axum server exited with an error")?;

        Ok::<(), anyhow::Error>(())
    })?;
    Ok(())
}
