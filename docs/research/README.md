# fynance Research

Research conducted April 2026 for the fynance project: a personal finance tracker written in Rust that ingests bank statements, categorizes transactions using Claude, and integrates with Obsidian for budgeting.

## Files

| File | Topic |
|---|---|
| [01_statement_formats.md](01_statement_formats.md) | Bank export formats, field schemas, bank-specific quirks |
| [02_rust_crates.md](02_rust_crates.md) | Rust crates for parsing CSV, OFX/QFX, and PDF statements |
| [03_categorization.md](03_categorization.md) | Rule-based, ML, and LLM-based transaction categorization |
| [04_data_storage_obsidian.md](04_data_storage_obsidian.md) | Storage options and Obsidian integration strategies |
| [05_budgeting_methodologies.md](05_budgeting_methodologies.md) | 50/30/20, zero-based, envelope methods, and projection |
| [06_obsidian_plugins.md](06_obsidian_plugins.md) | Plugin ecosystem: Dataview, SQLite DB, Templater, Charts |
| [07_claude_api.md](07_claude_api.md) | Claude API from Rust: tool use, batch, vision, prompt caching |

## Key Findings

1. **Statement ingestion**: Normalize all bank formats to a single internal `Transaction` struct using `csv` + a hand-rolled OFX parser (or `sgmlish`) + `pdf-extract`. Banks use CSV, OFX/QFX, and PDF with inconsistent field names and date formats.

2. **Categorization**: A hybrid rule-based (`regex`) + Claude API approach achieves ~94% accuracy. Rules handle 60-70% of transactions for free; Claude handles the ambiguous remainder via the batch endpoint (50% cheaper).

3. **Storage**: SQLite via `rusqlite` in the Obsidian vault is the best balance of performance and query power. Dataview covers markdown-native queries; the SQLite DB Obsidian plugin adds SQL plus charts inside notes.

4. **Budgeting**: Zero-based budgeting (every dollar assigned) is most powerful when combined with historical trend analysis. Claude (Sonnet) can generate monthly budget recommendations from categorized history.

5. **Obsidian integration**: Dataview + SQLite DB plugin + Charts is the recommended plugin stack. The Rust CLI writes to the database and optionally generates monthly report markdown; Obsidian renders live queries.

6. **Rust considerations**: Rust has excellent CSV and HTTP support but weaker PDF table extraction than Python's pdfplumber. For difficult PDFs, use Claude vision as the fallback rather than chasing a perfect Rust table parser.
