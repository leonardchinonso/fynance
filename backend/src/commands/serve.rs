//! `fynance serve` — start the local Axum HTTP server.
//!
//! This is the primary entrypoint for the app: the user runs one
//! command, the binary opens the default browser, and all further
//! interaction happens over HTTP on loopback. Phase 2 establishes the
//! server scaffold; later phases plug real endpoints into the router.

use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use tokio::net::TcpListener;

use crate::server;
use crate::storage::Db;

const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 7433;

/// FYNANCE_HOST may be a bare IPv4 or IPv6 address ("127.0.0.1", "::1").
/// `SocketAddr` parsing alone rejects unbracketed IPv6, so try `IpAddr`
/// first; the "{host}:{port}" fallback still accepts bracketed forms like
/// "[::1]" and errors on anything else (hostnames are not resolved).
fn resolve_bind_addr(host: &str, port: u16) -> Result<SocketAddr> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(SocketAddr::new(ip, port));
    }
    format!("{host}:{port}")
        .parse()
        .with_context(|| format!("parsing {host}:{port} as a socket address"))
}

/// User-facing URL for the printed banner and browser launch. Unspecified
/// binds (0.0.0.0, ::) display as `localhost` so copy/paste works across
/// operating systems; IPv6 hosts are bracketed so the URL stays valid.
fn browser_url(addr: &SocketAddr) -> String {
    let port = addr.port();
    if addr.ip().is_unspecified() {
        return format!("http://localhost:{port}");
    }
    match addr.ip() {
        IpAddr::V6(ip) => format!("http://[{ip}]:{port}"),
        IpAddr::V4(ip) => format!("http://{ip}:{port}"),
    }
}

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

        let addr = resolve_bind_addr(&host, port)?;

        let loopback_only = addr.ip().is_loopback();
        let app = server::build_router(Arc::new(Mutex::new(db)), loopback_only);

        let listener = TcpListener::bind(addr)
            .await
            .with_context(|| format!("binding tcp listener on {addr}"))?;

        let url = browser_url(&addr);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_bind_addr_ipv4() {
        let addr = resolve_bind_addr("127.0.0.1", 7433).unwrap();
        assert_eq!(addr, "127.0.0.1:7433".parse().unwrap());
        assert!(addr.ip().is_loopback());
    }

    #[test]
    fn resolve_bind_addr_ipv6_unbracketed() {
        let addr = resolve_bind_addr("::1", 7433).unwrap();
        assert_eq!(addr, "[::1]:7433".parse().unwrap());
        assert!(addr.ip().is_loopback());
    }

    #[test]
    fn resolve_bind_addr_ipv6_bracketed() {
        let addr = resolve_bind_addr("[::1]", 7433).unwrap();
        assert_eq!(addr, "[::1]:7433".parse().unwrap());
    }

    #[test]
    fn resolve_bind_addr_default_host() {
        let addr = resolve_bind_addr(DEFAULT_HOST, DEFAULT_PORT).unwrap();
        assert_eq!(addr, "127.0.0.1:7433".parse().unwrap());
        assert!(addr.ip().is_loopback());
    }

    #[test]
    fn resolve_bind_addr_rejects_hostnames() {
        assert!(resolve_bind_addr("localhost", 7433).is_err());
    }

    #[test]
    fn browser_url_display_forms() {
        let v4: SocketAddr = "127.0.0.1:7433".parse().unwrap();
        assert_eq!(browser_url(&v4), "http://127.0.0.1:7433");

        let any_v4: SocketAddr = "0.0.0.0:7433".parse().unwrap();
        assert_eq!(browser_url(&any_v4), "http://localhost:7433");

        let v6: SocketAddr = "[::1]:7433".parse().unwrap();
        assert_eq!(browser_url(&v6), "http://[::1]:7433");

        let any_v6: SocketAddr = "[::]:7433".parse().unwrap();
        assert_eq!(browser_url(&any_v6), "http://localhost:7433");
    }
}
