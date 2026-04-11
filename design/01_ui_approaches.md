# UI Approach Comparison

## Context

The original plan used Obsidian as the UI layer via the SQLite DB plugin. Prompt 1.1 requires a purpose-built UI with good visuals, a budget tab, and a portfolio overview. This document compares the viable approaches.

---

## Approach A: Axum Local Web Server + React/Svelte Frontend

**Architecture**: Rust (Axum) runs a local HTTP server. A compiled static frontend (React + Vite) is embedded in the binary via `include_dir!`. User runs `fynance serve` and the browser opens automatically.

```
[fynance serve]
      │
      ├─► Axum HTTP server (localhost:PORT)
      │     ├─ /api/transactions   ← REST JSON
      │     ├─ /api/budget
      │     ├─ /api/portfolio
      │     └─ /assets/*           ← embedded static files
      │
      └─► Spawns browser → localhost:PORT
```

**Pros**
- Best-in-class charts and UI components from the JS ecosystem (Recharts, Chart.js, Tremor, shadcn-ui)
- Full browser DevTools for debugging frontend
- React/Svelte/Vue are all mature with large component ecosystems
- No desktop packaging required; works on any OS with no install
- Port binding is local-only — no network exposure by default
- Rust backend stays clean; frontend is a separate project

**Cons**
- Requires Node.js toolchain to build the frontend (not at runtime)
- Two languages: Rust + JS/TS. Team members need both.
- `include_dir!` embedding adds binary size (typically 1-5 MB for a Vite bundle)
- No OS-level window decorations; feels like a browser app, not a native app
- Users must not have another process on the same port

**Verdict**: Best tradeoff for MVP. Frontend ecosystem dominates for charts and UX. Rust backend handles all data. Build outputs a single binary — no runtime Node.js needed.

---

## Approach B: Tauri (Rust Backend + WebView Desktop App)

**Architecture**: Tauri wraps a web frontend in a native OS webview. Rust is the backend; JS/TS (React/Svelte) is the frontend. Ships as a desktop `.app` / `.exe`.

```
[Tauri App]
  ├─ OS WebView (native window)
  │    └─ React/Svelte frontend
  └─ Rust backend (commands via IPC)
       └─ SQLite
```

**Pros**
- Native desktop window — feels like a proper app
- OS-level process isolation (each user installs their own app instance)
- Access to OS APIs (keychain for secrets, file pickers)
- Same JS frontend ecosystem as Approach A
- Ships as a single installable package

**Cons**
- Requires Node.js AND Rust toolchain at build time
- Tauri IPC (commands/events) adds boilerplate vs plain HTTP
- WebView rendering differences across OS (Safari/WebKit on macOS, Edge/Chromium on Windows)
- Harder to run headlessly / test end-to-end
- Distribution and updates require more infrastructure (tauri-updater)
- Heavier setup for an MVP

**Verdict**: Strong production choice but over-engineered for MVP. The packaging/IPC overhead is not justified until the product is stable.

---

## Approach C: egui / eframe (Pure Rust Immediate-Mode GUI)

**Architecture**: Single Rust binary with an immediate-mode GUI rendered via OpenGL/Metal/Vulkan.

```
[fynance]
  └─ eframe (native window)
       └─ egui widgets
            └─ egui_plot for charts
```

**Pros**
- Entire codebase in Rust — no JS/TS
- No webview, no browser dependency
- Compiles to a single binary with no runtime dependencies
- Hot-reload via `egui_extras`

**Cons**
- Immediate-mode paradigm is unfamiliar and verbose for complex layouts
- egui_plot offers basic line/bar charts — nothing close to Recharts/Chart.js in visual quality
- No CSS: layouts require manual sizing and spacing
- Limited accessibility support
- Fonts and typography are basic compared to HTML/CSS
- Custom components (tables, card grids, modals) require significant work

**Verdict**: Not suitable for "good visuals" and "good UX". Would require building from scratch what browsers give for free.

---

## Approach D: Dioxus (Rust + React-like, Desktop or WASM)

**Architecture**: Dioxus is a React-like framework in Rust. Can compile to desktop (via webview, like Tauri lite) or WASM web target.

**Pros**
- Full Rust — no JS/TS
- React-like component model (familiar paradigm)
- Single codebase can target web or desktop

**Cons**
- Ecosystem is much younger than React/Vue
- Charting libraries in Dioxus/WASM Rust are immature; would need to FFI into JS chart libs
- Desktop mode uses a webview (similar limitations to Tauri but without Tauri's polish)
- SSR and routing are still maturing
- Fewer UI component libraries vs shadcn/Radix/Tremor

**Verdict**: Promising long-term but not ready for a visually polished MVP today.

---

## Approach E: Leptos (Rust + WASM)

**Architecture**: Reactive Rust framework compiling to WASM for browser delivery.

**Pros**
- Full Rust
- Reactive signals (fine-grained reactivity, similar to SolidJS)
- Can share types between server and client

**Cons**
- WASM binary sizes can be large (50-200 KB+ gzipped, compile-time longer)
- Chart libraries: would need to call JS chart libs via wasm-bindgen — awkward
- Smaller community than React

**Verdict**: Interesting but adds WASM complexity without UI quality benefits for MVP.

---

## Recommendation: Approach A (Axum + React)

| Criterion | A (Axum+React) | B (Tauri) | C (egui) | D (Dioxus) |
|---|---|---|---|---|
| Visual quality | Excellent | Excellent | Poor | Moderate |
| UX quality | Excellent | Excellent | Poor | Moderate |
| MVP speed | Fast | Slow | Moderate | Slow |
| Single binary | Yes (embedded) | Yes | Yes | Yes |
| No runtime deps | Yes | Yes | Yes | Yes |
| Rust backend | Yes | Yes | Yes | Yes |
| Chart ecosystem | Recharts/Chart.js | Same | egui_plot | Limited |
| Multi-user isolation | Port per user | OS app | Process | Process |

**MVP stack**:
- Backend: Axum + rusqlite + tokio
- Frontend: React 18 + Vite + TypeScript + Recharts + shadcn-ui (Tailwind)
- Embedding: `include_dir!` macro compiles the Vite build into the binary
- Launch: `fynance serve [--port 3000]` — opens browser, binds only to `127.0.0.1`

**Future migration**: When the product is stable, wrapping the same Axum backend + React frontend in Tauri is a straightforward upgrade path. The API surface stays identical; only the distribution mechanism changes.

---

## Frontend Tooling Decision: React vs Svelte

Both are excellent. For MVP:

| | React | Svelte |
|---|---|---|
| Component libraries | shadcn-ui, Radix, Tremor (massive) | Skeleton, Flowbite-Svelte (smaller) |
| Chart libraries | Recharts, Victory, Chart.js wrappers | Chart.js wrappers, smaller ecosystem |
| Team familiarity | Very common | Less common |
| Bundle size | Larger | Smaller |

**Recommendation**: React + shadcn-ui + Recharts. shadcn-ui components are copy-paste (not a package dependency), which fits a Rust project that treats the frontend as an embedded asset.
