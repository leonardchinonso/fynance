# Updated System Architecture

## Overview

fynance is a single Rust binary that:
1. Serves a React web UI over a local-only HTTP server (Axum)
2. Processes bank CSV imports and categorizes transactions via Claude API
3. Stores all data in a per-user SQLite database
4. Provides four primary views: Transactions, Budget, Portfolio, and Reports

## Component Diagram

```
┌──────────────────────────────────────────────────────────────────┐
│                        User's Machine                             │
│                                                                   │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │                    fynance binary                           │ │
│  │                                                             │ │
│  │  ┌─────────────┐    ┌──────────────────────────────────┐   │ │
│  │  │  CLI Layer   │    │         Axum HTTP Server          │   │ │
│  │  │  (clap)      │    │         (127.0.0.1:PORT)          │   │ │
│  │  │              │    │                                   │   │ │
│  │  │  import      │    │  GET  /api/transactions           │   │ │
│  │  │  categorize  │    │  GET  /api/budget/:month          │   │ │
│  │  │  serve       │    │  GET  /api/portfolio              │   │ │
│  │  │  export      │    │  POST /api/import                 │   │ │
│  │  │  budget      │    │  POST /api/categorize             │   │ │
│  │  └──────┬───────┘    │  POST /api/accounts               │   │ │
│  │         │            │  GET  /assets/* (embedded React)  │   │ │
│  │         │            └───────────────┬──────────────────┘   │ │
│  │         │                            │                       │ │
│  │         └──────────────┬─────────────┘                       │ │
│  │                        │                                     │ │
│  │              ┌─────────▼──────────┐                         │ │
│  │              │   Core Services    │                         │ │
│  │              │                    │                         │ │
│  │              │  importer          │                         │ │
│  │              │  categorizer       │                         │ │
│  │              │  budget_engine     │                         │ │
│  │              │  portfolio_tracker │                         │ │
│  │              │  report_generator  │                         │ │
│  │              └─────────┬──────────┘                         │ │
│  │                        │                                     │ │
│  │              ┌─────────▼──────────┐                         │ │
│  │              │  Storage Layer     │                         │ │
│  │              │  (rusqlite)        │                         │ │
│  │              │                   │                         │ │
│  │              │  fynance.db        │                         │ │
│  │              │  ~/.local/share/   │                         │ │
│  │              │  fynance/          │                         │ │
│  │              └────────────────────┘                         │ │
│  └─────────────────────────────────────────────────────────────┘ │
│                              ▲                                    │
│                              │ HTTP (loopback only)               │
│  ┌───────────────────────────┴──────────────────────────────┐    │
│  │              Browser (user's default browser)            │    │
│  │              React + Recharts + shadcn-ui                │    │
│  │                                                          │    │
│  │  [Transactions] [Budget] [Portfolio] [Reports]           │    │
│  └──────────────────────────────────────────────────────────┘    │
│                                                                   │
│  External: Claude API (HTTPS, optional, for categorization)       │
└──────────────────────────────────────────────────────────────────┘
```

## Module Layout

```
fynance/
├── Cargo.toml
├── sql/
│   └── schema.sql
├── config/
│   ├── categories.yaml
│   └── rules.yaml
├── src/
│   ├── main.rs
│   ├── cli.rs                   # clap subcommand definitions
│   ├── model.rs                 # Transaction, Account, Budget types
│   │
│   ├── importers/
│   │   ├── mod.rs               # Importer trait
│   │   ├── csv_importer.rs      # Generic CSV with bank mappings
│   │   ├── monzo.rs             # Monzo-specific mapping
│   │   ├── revolut.rs           # Revolut-specific mapping
│   │   └── lloyds.rs            # Lloyds-specific mapping
│   │
│   ├── categorizer/
│   │   ├── mod.rs
│   │   ├── rules.rs             # YAML rule-based categorization
│   │   ├── claude.rs            # Claude API integration
│   │   └── pipeline.rs          # rule-first then Claude
│   │
│   ├── budget/
│   │   ├── mod.rs
│   │   ├── analyzer.rs          # Budget vs actual calculations
│   │   └── advisor.rs           # Claude-generated budget suggestions
│   │
│   ├── portfolio/
│   │   ├── mod.rs
│   │   ├── accounts.rs          # Account balance tracking
│   │   └── diversity.rs         # Diversity calculations
│   │
│   ├── storage/
│   │   ├── mod.rs
│   │   └── db.rs                # Db struct, all SQL queries
│   │
│   ├── server/
│   │   ├── mod.rs               # Axum router setup
│   │   ├── routes/
│   │   │   ├── transactions.rs
│   │   │   ├── budget.rs
│   │   │   ├── portfolio.rs
│   │   │   └── import.rs
│   │   └── static_files.rs      # include_dir! embedded frontend
│   │
│   └── util.rs                  # normalize_description, fingerprint, parse_date
│
└── frontend/
    ├── package.json             # React + Vite + shadcn-ui + Recharts
    ├── src/
    │   ├── main.tsx
    │   ├── App.tsx
    │   ├── pages/
    │   │   ├── Transactions.tsx
    │   │   ├── Budget.tsx
    │   │   ├── Portfolio.tsx
    │   │   └── Reports.tsx
    │   ├── components/
    │   │   ├── TransactionTable.tsx
    │   │   ├── SpendingChart.tsx
    │   │   ├── BudgetProgress.tsx
    │   │   ├── PortfolioCard.tsx
    │   │   └── DiversityPieChart.tsx
    │   └── api/
    │       └── client.ts        # fetch wrappers for Axum endpoints
    └── dist/                    # compiled output, embedded by Rust
```

## Data Flow: CSV Import

```
User: fynance import monzo_2024.csv --account monzo-current
  │
  ├─► CLI parses args
  ├─► get_importer("monzo") → MonzoImporter
  ├─► CsvImporter::parse() → Iterator<Transaction>
  │     each record:
  │       normalize_description()
  │       fingerprint()
  ├─► storage::insert_transaction() → Inserted | Duplicate
  ├─► storage::log_import()
  └─► Print summary: "Imported 142 new, 0 duplicates"
```

## Data Flow: Serve Mode

```
User: fynance serve
  │
  ├─► Db::open() → verify schema migrations
  ├─► Axum router::new() with all routes
  ├─► Bind to 127.0.0.1:3000 (or $PORT)
  ├─► spawn_browser("http://localhost:3000")
  └─► Server loop

Browser → GET / → serve embedded index.html (React app)
Browser → GET /api/transactions?month=2026-03 → JSON array
Browser → GET /api/budget/2026-03 → budget vs actual JSON
Browser → GET /api/portfolio → accounts + diversity JSON
```

## API Shape (REST JSON)

### GET /api/transactions
Query params: `month`, `category`, `account`, `page`, `limit`

```json
{
  "transactions": [
    {
      "id": "uuid",
      "date": "2026-03-15",
      "description": "MONZO MERCHANT",
      "normalized": "Monzo Merchant",
      "amount": "-42.50",
      "currency": "GBP",
      "category": "Food: Dining",
      "account_id": "monzo-current",
      "confidence": 0.95
    }
  ],
  "total": 314,
  "page": 1,
  "limit": 50
}
```

### GET /api/budget/:month
```json
{
  "month": "2026-03",
  "income": "4500.00",
  "categories": [
    {
      "name": "Food: Groceries",
      "budgeted": "300.00",
      "actual": "278.42",
      "percent_used": 92.8
    }
  ],
  "total_budgeted": "3200.00",
  "total_actual": "2940.18"
}
```

### GET /api/portfolio
```json
{
  "accounts": [
    {
      "id": "monzo-current",
      "name": "Monzo Current",
      "type": "checking",
      "balance": "1240.00",
      "currency": "GBP",
      "last_updated": "2026-04-10"
    }
  ],
  "net_worth": "28450.00",
  "by_type": [
    { "type": "checking", "total": "2140.00", "percent": 7.5 },
    { "type": "savings", "total": "12000.00", "percent": 42.2 },
    { "type": "investments", "total": "14310.00", "percent": 50.3 }
  ]
}
```

## Design Principles

1. **Loopback only**: Axum binds to `127.0.0.1`, never `0.0.0.0`. No LAN exposure.
2. **Single binary**: `cargo build --release` embeds the React bundle; no separate install.
3. **Offline-first**: The UI works fully offline. Claude API calls are background-optional.
4. **Per-user isolation**: DB path resolves from the OS user's home directory. See security doc.
5. **Deduplication on import**: Re-importing the same file is always safe.
6. **No raw descriptions in logs**: Merchant names stay out of INFO-level logs.
