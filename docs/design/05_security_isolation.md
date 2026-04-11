# Security and User Isolation

## Threat Model

fynance is a single-user local application. It handles sensitive financial data. The primary concerns are:

1. **Data confidentiality**: Other users on the same OS must not access another user's financial data.
2. **Network exposure**: The local web server must not be accessible from the LAN or internet.
3. **API key security**: The Claude API key must not be stored in plaintext in a world-readable location.
4. **No telemetry**: No financial data must leave the machine except for Claude API categorization calls, which must be opt-in.

This is NOT a multi-user SaaS system. "Multi-user" here means multiple people sharing the same machine, each running their own isolated fynance instance.

---

## Storage Isolation

### Database Path

The SQLite database path resolves from the OS user's home directory using the `dirs` crate:

```rust
// src/storage/db.rs
use dirs::data_local_dir;

pub fn default_db_path() -> PathBuf {
    data_local_dir()
        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
        .join("fynance")
        .join("fynance.db")
}
```

| Platform | Resolved Path |
|---|---|
| macOS | `~/Library/Application Support/fynance/fynance.db` |
| Linux | `~/.local/share/fynance/fynance.db` |
| Windows | `%APPDATA%\fynance\fynance.db` |

The directory is created with mode `700` (owner-only) on Unix:

```rust
use std::fs;
use std::os::unix::fs::DirBuilderExt;

fn ensure_data_dir(path: &Path) -> anyhow::Result<()> {
    if !path.exists() {
        #[cfg(unix)]
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)?;

        #[cfg(not(unix))]
        fs::create_dir_all(path)?;
    }
    Ok(())
}
```

The database file itself is created with mode `600` after the first `PRAGMA` call. SQLite on Unix honors the file permissions set by the process umask; setting umask `077` before opening is sufficient for most cases. An explicit `chmod` after creation adds a belt-and-suspenders guarantee.

### Config Path

Category rules and account mappings are stored in:
- macOS/Linux: `~/.config/fynance/`
- Windows: `%APPDATA%\fynance\config\`

---

## Network Isolation

### Loopback-Only Binding

The Axum server binds to `127.0.0.1` (IPv4 loopback), never `0.0.0.0`:

```rust
// src/server/mod.rs
let addr = SocketAddr::new(
    IpAddr::V4(Ipv4Addr::LOCALHOST),   // 127.0.0.1
    port,
);
let listener = tokio::net::TcpListener::bind(addr).await?;
```

This ensures:
- The server is unreachable from the LAN or internet
- Other machines on the same network cannot connect
- Only processes on the same OS can connect (and only the user's browser will)

### Port Selection

Default port: `7433`. If taken, the server tries `7434`..`7442` and reports which port it bound to:

```
fynance: server started at http://localhost:3001
```

The port can be overridden: `fynance serve --port 4000`.

### No CORS for External Origins

The Axum CORS layer is configured to allow only `http://localhost:<port>`:

```rust
use tower_http::cors::{CorsLayer, Any};

let cors = CorsLayer::new()
    .allow_origin(format!("http://localhost:{}", port).parse::<HeaderValue>().unwrap())
    .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::DELETE])
    .allow_headers(Any);
```

---

## API Key Security

The Claude API key is read from an environment variable (`ANTHROPIC_API_KEY`) or from a config file at `~/.config/fynance/config.yaml` with mode `600`.

```yaml
# ~/.config/fynance/config.yaml (chmod 600)
claude_api_key: sk-ant-...
```

**The key is never**:
- Hardcoded in source
- Stored in the SQLite database
- Logged at any level
- Sent to any endpoint other than `api.anthropic.com`

If the key is absent, categorization falls back to rules-only mode and a warning is shown.

---

## Claude API Data Minimization

When sending transactions to Claude for categorization:

1. Only the **normalized description** and **amount category** (debit/credit, rough magnitude) are sent — not the exact amount, not the date, not the account ID.
2. Batch mode (Claude Batch API) is preferred: fewer API calls, no real-time data streaming.
3. Users can opt out of Claude categorization entirely with `--no-ai` flag.

Example prompt sent to Claude (no raw financial data):
```
Categorize this transaction description into one of the following categories.
Description: "LIDL GB LONDON"
Categories: [Food: Groceries, Food: Dining & Bars, Shopping: General, ...]
Return only the category name.
```

---

## Authentication

### Browser UI (No Auth)

Browser requests from localhost require no authentication. The loopback binding is the trust boundary, same as local dev servers (Vite, Rails, Django). Anyone who can reach `localhost` is the OS user.

### Programmatic API Access (Token Auth)

Scripts, agents, and automation tools authenticate via bearer tokens. Tokens are generated locally via the CLI and stored as SHA-256 hashes in the database.

```bash
fynance token create --name "import-script"
# Token: fyn_a1b2c3d4e5f6...  (displayed once)
```

Usage:
```bash
curl -H "Authorization: Bearer fyn_..." http://localhost:7433/api/import/csv -F file=@data.csv
```

Token security:
- Tokens are prefixed with `fyn_` for easy identification in logs and config
- Only the SHA-256 hash is stored in the DB, never the raw token
- Tokens can be revoked via `fynance token revoke --name <name>`
- Token last-used timestamp is recorded for auditing
- Tokens are scoped to the local instance only

This allows agents (e.g., a Claude Code script) to push CSV files or screenshots to the API without exposing the full UI.

---

## Future Improvements

Potential security enhancements for later phases, once the MVP is stable:

1. **Optional browser UI authentication**: Add an optional password or PIN gate for the web UI. Currently loopback binding is the trust boundary, but this would add protection in shared-machine scenarios or when running in Docker with `0.0.0.0` binding. Options include:
   - A random token embedded in the server URL at startup (e.g., `http://localhost:7433/?token=<random>`)
   - HTTP Basic Auth with a locally-generated password stored in the OS keychain
   - A simple session cookie with a locally-set password or PIN

2. **HTTPS support**: Add optional TLS termination for the Axum server, useful when accessing the Docker deployment over a LAN or through a reverse proxy.

3. **Token scoping**: Restrict API tokens to specific endpoints or actions (e.g., read-only tokens, import-only tokens) rather than full API access.

4. **Audit log**: Record all data-modifying operations (imports, category changes, balance updates) with timestamps and source (UI, API token name, CLI).

---

## Summary Checklist

| Concern | Mitigation |
|---|---|
| DB readable by other OS users | `chmod 700` on data dir, `chmod 600` on DB file |
| Server accessible from LAN | Bind to `127.0.0.1` only |
| CORS from other origins | CORS limited to `localhost:<port>` |
| API key exposure | Env var or `chmod 600` config file; never logged or stored in DB |
| Raw transaction data sent to Claude | Only normalized descriptions, never amounts or dates |
| Programmatic API access | Bearer token auth; tokens stored as SHA-256 hashes |
| Single port for UI + API | One port serves the web UI and REST API. No need for multiple exposed ports. |
| Telemetry | None. No calls except explicit Claude API categorization. |
