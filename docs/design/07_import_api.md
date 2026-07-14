# Import API: 2-Stage Architecture

This document specifies the API contracts for the 2-stage import system. Stage 1 parses uploaded documents and returns a structured preview. Stage 2 commits the previewed data to the database. Nothing is written to the database during Stage 1.

All TypeScript types referenced below are auto-generated from Rust via `ts-rs` and live in `frontend/src/bindings/`.

---

## How It Works

```
  Upload files                    Review preview                   Commit data
 ┌──────────┐               ┌──────────────────┐           ┌─────────────────────┐
 │  User     │  POST /parse  │  Backend parses  │  Returns   │  Frontend shows     │
 │  selects  │ ────────────> │  files via LLM   │ ────────> │  preview tables     │
 │  files    │               │  (no DB writes)  │           │  for each data type │
 └──────────┘               └──────────────────┘           └────────┬────────────┘
                                                                     │
                                                          User clicks "Confirm"
                                                                     │
                                                     ┌───────────────┼───────────────┐
                                                     ▼               ▼               ▼
                                              POST /import   POST /holdings   POST /investments
                                              /transactions     /import           /import
                                                     │               │               │
                                                     ▼               ▼               ▼
                                                  Data committed to database
```

1. The frontend sends files to `POST /api/parse`.
2. The backend returns an `IngestionPreview` containing previews and payloads for transactions, holdings, and investments.
3. The frontend renders a preview table for each data type that has results.
4. When the user confirms, the frontend sends each `payload` to the corresponding Stage 2 endpoint.
5. Each Stage 2 endpoint commits data and returns a result summary.

The frontend orchestrates the flow. The backend is stateless between Stage 1 and Stage 2: the preview response contains everything needed to commit. There are no tokens, sessions, or server-side caches.

---

## Stage 1: Parse Documents

### `POST /api/parse`

Accepts one or more file uploads. Returns a structured preview of all data found across the files. Never writes to the database.

**Content-Type:** `multipart/form-data`

#### Request Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `files[]` | File | Yes (at least 1) | One or more files to parse. Supported formats: CSV, PDF, XLSX. Use the field name `files[]` for multiple files or `file` for a single file. |
| `account_id` | string | No | Target account ID. If provided, all extracted data is associated with this account. If omitted, the parser may detect it from the file content. |
| `hints` | string (JSON) | No | Optional guidance to improve parsing accuracy. See Hints Object below. |

#### Hints Object

When provided, `hints` must be a JSON string with any of these fields:

```json
{
  "institution": "monzo",
  "expected_data": ["transactions", "holdings"],
  "date_format": "DD/MM/YYYY"
}
```

| Field | Type | Description |
|-------|------|-------------|
| `institution` | string | Institution name (e.g., `"monzo"`, `"revolut"`, `"trading_212"`). Helps the parser when auto-detection fails. |
| `expected_data` | string[] | Hint about what type of data the files contain. Values: `"transactions"`, `"holdings"`, `"investments"`. |
| `date_format` | string | Date format used in the files if non-standard. |

#### Response

**Status:** `200 OK`

**Type:** `IngestionPreview`

```typescript
type IngestionPreview = {
  status: IngestionStatus;
  metadata: IngestionMetadata;
  transactions: TransactionIngestionResult;
  holdings: HoldingsIngestionResult;
  investments: InvestmentIngestionResult;
  clarifications_needed: ClarificationRequest[];
};

type IngestionStatus = "success" | "needs_clarification" | "error";

type IngestionMetadata = {
  files_processed: number;
  institution_detected: string | null;
  detection_confidence: number;       // 0.0 to 1.0
  processing_time_ms: number;
  notes: string[];                    // informational messages from the parser
  relationships_found: string[];      // cross-document relationships detected
};
```

**`transactions`:**

```typescript
type TransactionIngestionResult = {
  count: number;       // total rows found
  new: number;         // rows not in the database
  duplicate: number;   // rows already in the database (by fingerprint)
  errors: number;      // rows that failed to parse
  rows: TransactionPreviewRow[];
  payload: ImportPayload | null;  // send this to POST /api/import to commit
};

type TransactionPreviewRow = {
  index: number;
  date: string;                          // ISO 8601 datetime
  description: string;
  amount: string;                        // decimal as string, negative = outflow
  currency: string;
  status: "new" | "duplicate" | "error";
  existing_id: string | null;            // if duplicate, the ID of the existing row
  existing_description: string | null;   // if duplicate, the description of the existing row
  error_reason: string | null;           // if error, why
  source_document_ids: string[];         // documents this row was extracted from
};
```

**`holdings`:**

```typescript
type HoldingsIngestionResult = {
  count: number;       // total holdings found
  new: number;         // holdings not in the database
  modify: number;      // holdings that exist but with different values
  rows: HoldingPreview[];
  payload: HoldingsImportPayload | null;  // send this to POST /api/holdings/import
};

type HoldingPreview = {
  account_id: string;
  symbol: string;
  sub_account: string | null;
  value: string;                   // decimal as string
  currency: string;
  as_of: string;                   // ISO 8601 datetime
  status: string;                  // "new" or "modify"
  existing_value: string | null;   // current value in DB if status is "modify"
};
```

**`investments`:**

```typescript
type InvestmentIngestionResult = {
  count: number;       // total investment events found
  new: number;         // events not in the database
  duplicate: number;   // events already in the database (by fingerprint)
  rows: InvestmentPreviewRow[];
  payload: InvestmentsImportPayload | null;  // send this to POST /api/investments/import
};

type InvestmentPreviewRow = {
  index: number;
  event_type: string;               // "buy", "sell", "vest", "dividend", etc.
  symbol: string;
  date: string;                     // ISO 8601 datetime
  quantity: string;                 // decimal as string
  price_per_share: string;          // decimal as string
  currency: string;
  status: "new" | "duplicate" | "error";
  existing_id: string | null;       // if duplicate, the ID of the existing event
  source_document_ids: string[];    // documents this event was extracted from
};
```

**`clarifications_needed`** (only present when `status` is `"needs_clarification"`):

```typescript
type ClarificationRequest = {
  file: string;          // filename that needs clarification
  question: string;      // human-readable question for the user
  suggestions: string[]; // suggested answers the UI can show as buttons/chips
};
```

#### Error Responses

| Status | Condition | Body |
|--------|-----------|------|
| `400` | No files provided | `{ "error": "at least one file is required", "code": "no_files" }` |
| `400` | Malformed multipart | `{ "error": "multipart error: ...", "code": "invalid_multipart" }` |
| `400` | File read failure | `{ "error": "failed to read file: ...", "code": "file_read_error" }` |

#### Example: Single CSV Upload

**Request:**

```bash
curl -X POST http://127.0.0.1:7433/api/parse \
  -F "files[]=@monzo_may_2026.csv" \
  -F "account_id=acc_monzo_main"
```

**Response:**

```json
{
  "status": "success",
  "metadata": {
    "files_processed": 1,
    "institution_detected": "monzo",
    "detection_confidence": 0.97,
    "processing_time_ms": 2340,
    "notes": [],
    "relationships_found": []
  },
  "transactions": {
    "count": 47,
    "new": 12,
    "duplicate": 35,
    "errors": 0,
    "rows": [
      {
        "index": 0,
        "date": "2026-05-01T00:00:00",
        "description": "Lidl",
        "amount": "-23.45",
        "currency": "GBP",
        "status": "duplicate",
        "existing_id": "tx_abc123",
        "existing_description": "Lidl"
      },
      {
        "index": 35,
        "date": "2026-05-15T00:00:00",
        "description": "TfL",
        "amount": "-2.80",
        "currency": "GBP",
        "status": "new",
        "existing_id": null,
        "existing_description": null
      }
    ],
    "payload": {
      "account_id": "acc_monzo_main",
      "transactions": [
        {
          "date": "2026-05-15T00:00:00",
          "description": "TfL",
          "amount": "-2.80",
          "currency": "GBP",
          "category": "Transport",
          "category_id": null,
          "category_source": "rule",
          "notes": null,
          "is_recurring": null,
          "exclude_from_summary": null
        }
      ]
    }
  },
  "holdings": {
    "count": 0,
    "new": 0,
    "modify": 0,
    "rows": [],
    "payload": null
  },
  "investments": {
    "count": 0,
    "new": 0,
    "duplicate": 0,
    "rows": [],
    "payload": null
  }
}
```

#### Example: Multi-File Upload (Holdings + Investment History)

**Request:**

```bash
curl -X POST http://127.0.0.1:7433/api/parse \
  -F "files[]=@T212_Positions_2026-05-17.csv" \
  -F "files[]=@T212_Transaction_History_2026.csv" \
  -F "account_id=acc_t212_isa" \
  -F 'hints={"institution": "trading_212"}'
```

**Response:**

```json
{
  "status": "success",
  "metadata": {
    "files_processed": 2,
    "institution_detected": "trading_212",
    "detection_confidence": 0.95,
    "processing_time_ms": 4120,
    "notes": [
      "File 1 contains a holdings snapshot with 15 positions.",
      "File 2 contains 28 buy/sell events spanning Jan-May 2026."
    ],
    "relationships_found": [
      "12 of 15 holdings have corresponding buy events in the transaction history."
    ]
  },
  "transactions": {
    "count": 0,
    "new": 0,
    "duplicate": 0,
    "errors": 0,
    "rows": [],
    "payload": null
  },
  "holdings": {
    "count": 15,
    "new": 3,
    "modify": 12,
    "rows": [
      {
        "account_id": "acc_t212_isa",
        "symbol": "VUSA",
        "sub_account": null,
        "value": "3816.00",
        "currency": "GBP",
        "as_of": "2026-05-17T00:00:00",
        "status": "modify",
        "existing_value": "3654.00"
      },
      {
        "account_id": "acc_t212_isa",
        "symbol": "AAPL",
        "sub_account": null,
        "value": "1984.50",
        "currency": "USD",
        "as_of": "2026-05-17T00:00:00",
        "status": "new",
        "existing_value": null
      }
    ],
    "payload": {
      "account_id": "acc_t212_isa",
      "holdings": [
        {
          "account_id": "acc_t212_isa",
          "symbol": "VUSA",
          "name": "Vanguard S&P 500 UCITS ETF",
          "holding_type": "etf",
          "quantity": "50.0000",
          "price_per_unit": "76.32",
          "value": "3816.00",
          "currency": "GBP",
          "as_of": "2026-05-17T00:00:00",
          "short_name": "VUSA",
          "sub_account": null,
          "is_closed": false
        },
        {
          "account_id": "acc_t212_isa",
          "symbol": "AAPL",
          "name": "Apple Inc",
          "holding_type": "stock",
          "quantity": "10.0000",
          "price_per_unit": "198.45",
          "value": "1984.50",
          "currency": "USD",
          "as_of": "2026-05-17T00:00:00",
          "short_name": "AAPL",
          "sub_account": null,
          "is_closed": false
        }
      ]
    }
  },
  "investments": {
    "count": 28,
    "new": 5,
    "duplicate": 23,
    "rows": [
      {
        "index": 0,
        "event_type": "buy",
        "symbol": "AAPL",
        "date": "2026-04-10T14:30:00",
        "quantity": "10.0000",
        "price_per_share": "185.20",
        "currency": "USD",
        "status": "new",
        "existing_id": null
      },
      {
        "index": 1,
        "event_type": "buy",
        "symbol": "VUSA",
        "date": "2026-01-15T09:00:00",
        "quantity": "5.0000",
        "price_per_share": "72.10",
        "currency": "GBP",
        "status": "duplicate",
        "existing_id": "inv_xyz789"
      }
    ],
    "payload": {
      "account_id": "acc_t212_isa",
      "events": [
        {
          "account_id": "acc_t212_isa",
          "event_type": "buy",
          "symbol": "AAPL",
          "date": "2026-04-10T14:30:00",
          "quantity": "10.0000",
          "price_per_share": "185.20",
          "fee": "0.00",
          "currency": "USD",
          "notes": null
        }
      ]
    }
  }
}
```

#### Example: Low Confidence (Needs Clarification)

**Request:**

```bash
curl -X POST http://127.0.0.1:7433/api/parse \
  -F "files[]=@export_123.xlsx"
```

**Response:**

```json
{
  "status": "needs_clarification",
  "metadata": {
    "files_processed": 1,
    "institution_detected": null,
    "detection_confidence": 0.42,
    "processing_time_ms": 3200
  },
  "clarifications_needed": [
    {
      "file": "export_123.xlsx",
      "question": "This file contains tabular financial data but the institution could not be identified. Which institution is this from?",
      "suggestions": ["trading_212", "freetrade", "aj_bell", "hargreaves_lansdown"]
    }
  ],
  "transactions": {
    "count": 0,
    "new": 0,
    "duplicate": 0,
    "errors": 0,
    "rows": [],
    "payload": null
  },
  "holdings": {
    "count": 0,
    "new": 0,
    "modify": 0,
    "rows": [],
    "payload": null
  },
  "investments": {
    "count": 0,
    "new": 0,
    "duplicate": 0,
    "rows": [],
    "payload": null
  }
}
```

When the frontend receives `"needs_clarification"`, it should display the questions to the user, collect answers, and retry the request with those answers as hints:

```bash
curl -X POST http://127.0.0.1:7433/api/parse \
  -F "files[]=@export_123.xlsx" \
  -F 'hints={"institution": "trading_212"}'
```

---

## Stage 2: Commit Endpoints

Each Stage 2 endpoint receives the `payload` field from the corresponding section of the `IngestionPreview` response. The frontend sends only the payloads the user has confirmed.

Stage 2 endpoints are independent. Call them in any order. If one fails, the others are unaffected.

> **Forward `payload` as-is. Do not rebuild it by hand.**
>
> Every row in a Stage 1 `payload` carries `source_document_ids`, linking it to the document it was extracted from. That is what populates the Source column and lets a stored figure be traced back to the statement it came from.
>
> Dropping or filtering rows is fine (`payload.events.filter(...)`), but reconstructing each row field-by-field is not: it is easy to omit `source_document_ids` and nothing will fail. The rows commit cleanly, and the provenance is simply gone. A bulk import that reconstructed its payload this way left 691 investment events with no source link and no error to show for it.
>
> If you must build a payload without a Stage 1 parse (a CSV you already hold, a correction script), upload the source file via `POST /api/documents` first and set `source_document_ids` to the returned id. Leave it empty only when there genuinely is no source document.

---

### `POST /api/import` (Transactions)

Commits transactions to the database. This endpoint already existed before the 2-stage redesign and remains unchanged. Send the `transactions.payload` from the parse response.

**Content-Type:** `application/json`

#### Request Body

**Type:** `ImportPayload`

```typescript
type ImportPayload = {
  account_id: string;
  transactions: ImportTransaction[];
};

type ImportTransaction = {
  date: string;                              // ISO 8601: "2026-05-15T00:00:00"
  description: string;
  amount: string;                            // decimal as string, negative = outflow
  currency: string | null;                   // ISO 4217 code, e.g. "GBP"
  category: string | null;                   // category name (legacy, for backward compat)
  category_id: string | null;                // preferred: FK to categories table
  category_source: "rule" | "agent" | "manual" | null;
  notes: string | null;
  is_recurring: boolean | null;
  exclude_from_summary: boolean | null;
};
```

#### Response

**Status:** `200 OK`

**Type:** `ImportResult`

```typescript
type ImportResult = {
  rows_total: number;
  rows_inserted: number;
  rows_duplicate: number;
  filename: string;         // "<api>" for JSON imports
  account_id: string;
  detected_bank: "monzo" | "revolut" | "lloyds" | "unknown";
  detection_confidence: number;
  errors: ImportRowError[];
};

type ImportRowError = {
  index: number;    // zero-based index into the transactions array
  reason: string;
};
```

#### Error Responses

| Status | Condition | Body |
|--------|-----------|------|
| `400` | Empty account_id | `{ "error": "account_id must not be empty", "code": "invalid_account_id" }` |
| `400` | Empty transactions array | `{ "error": "transactions array must not be empty", "code": "empty_transactions" }` |

#### Example

**Request:**

```bash
curl -X POST http://127.0.0.1:7433/api/import \
  -H "Content-Type: application/json" \
  -d '{
    "account_id": "acc_monzo_main",
    "transactions": [
      {
        "date": "2026-05-15T00:00:00",
        "description": "TfL",
        "amount": "-2.80",
        "currency": "GBP",
        "category": "Transport",
        "category_id": null,
        "category_source": "rule",
        "notes": null,
        "is_recurring": null,
        "exclude_from_summary": null
      },
      {
        "date": "2026-05-16T00:00:00",
        "description": "Pret A Manger",
        "amount": "-4.50",
        "currency": "GBP",
        "category": "Eating Out",
        "category_id": null,
        "category_source": "rule",
        "notes": null,
        "is_recurring": null,
        "exclude_from_summary": null
      }
    ]
  }'
```

**Response:**

```json
{
  "rows_total": 2,
  "rows_inserted": 2,
  "rows_duplicate": 0,
  "filename": "<api>",
  "account_id": "acc_monzo_main",
  "detected_bank": "unknown",
  "detection_confidence": 0.0,
  "errors": []
}
```

---

### `POST /api/holdings/import` (Holdings)

Commits holdings to the database. This endpoint already existed before the 2-stage redesign and remains unchanged. Send the `holdings.payload` from the parse response.

Do NOT include `?dry_run=true` when calling this from the Stage 2 flow. The preview already happened in Stage 1.

**Content-Type:** `application/json`

#### Request Body

**Type:** `HoldingsImportPayload`

```typescript
type HoldingsImportPayload = {
  account_id: string;
  holdings: Holding[];
};

type Holding = {
  account_id: string;
  symbol: string;
  name: string;
  holding_type: "stock" | "etf" | "fund" | "bond" | "crypto" | "cash" | "property" | "loan" | "credit";
  quantity: string;               // decimal as string
  price_per_unit: string | null;  // decimal as string, null if unknown
  value: string;                  // decimal as string, total value
  currency: string;               // ISO 4217 code
  as_of: string;                  // ISO 8601 datetime
  short_name: string | null;      // display abbreviation
  sub_account: string | null;     // disambiguates multiple holdings of the same symbol (e.g. pots)
  is_closed: boolean;             // true to archive without deleting
};
```

#### Response

**Status:** `200 OK`

```typescript
type HoldingsImportResult = {
  inserted: number;
  updated: number;
  total: number;
};
```

#### Error Responses

| Status | Condition | Body |
|--------|-----------|------|
| `400` | Empty account_id | `{ "error": "account_id is required", "code": "missing_account_id" }` |
| `400` | Empty holdings array | `{ "error": "holdings array must not be empty", "code": "empty_holdings" }` |

#### Example

**Request:**

```bash
curl -X POST http://127.0.0.1:7433/api/holdings/import \
  -H "Content-Type: application/json" \
  -d '{
    "account_id": "acc_t212_isa",
    "holdings": [
      {
        "account_id": "acc_t212_isa",
        "symbol": "VUSA",
        "name": "Vanguard S&P 500 UCITS ETF",
        "holding_type": "etf",
        "quantity": "50.0000",
        "price_per_unit": "76.32",
        "value": "3816.00",
        "currency": "GBP",
        "as_of": "2026-05-17T00:00:00",
        "short_name": "VUSA",
        "sub_account": null,
        "is_closed": false
      },
      {
        "account_id": "acc_t212_isa",
        "symbol": "AAPL",
        "name": "Apple Inc",
        "holding_type": "stock",
        "quantity": "10.0000",
        "price_per_unit": "198.45",
        "value": "1984.50",
        "currency": "USD",
        "as_of": "2026-05-17T00:00:00",
        "short_name": "AAPL",
        "sub_account": null,
        "is_closed": false
      }
    ]
  }'
```

**Response:**

```json
{
  "inserted": 1,
  "updated": 1,
  "total": 2
}
```

---

### `POST /api/investments/import` (Investments)

Bulk imports investment events. This is a new endpoint. Send the `investments.payload` from the parse response.

The endpoint iterates over each event, validates it, and inserts it into the database. Events that fail validation are recorded in the `errors` array; processing continues for the remaining events. Duplicate events (matching fingerprint) are silently deduplicated.

**Content-Type:** `application/json`

#### Request Body

**Type:** `InvestmentsImportPayload`

```typescript
type InvestmentsImportPayload = {
  account_id: string;
  events: CreateInvestmentEventBody[];
};

type CreateInvestmentEventBody = {
  account_id: string;         // overridden by the top-level account_id
  event_type: string;         // "buy" | "sell" | "vest" | "transfer" | "withhold" | "split"
  symbol: string;             // ticker symbol, e.g. "AAPL"
  date: string;               // ISO 8601 datetime
  quantity: string;           // decimal as string
  price_per_share: string;    // decimal as string
  fee: string | null;         // decimal as string, null if no fee
  currency: string;           // ISO 4217 code
  notes: string | null;
};
```

Note: the `account_id` on each event is overridden by the top-level `account_id` field. All events in a single request are imported to the same account.

#### Response

**Status:** `200 OK`

**Type:** `InvestmentImportResult`

```typescript
type InvestmentImportResult = {
  total: number;       // total events in the request
  inserted: number;    // events successfully inserted
  duplicates: number;  // events skipped as duplicates
  errors: InvestmentImportError[];
};

type InvestmentImportError = {
  index: number;    // zero-based index into the events array
  reason: string;   // human-readable error message
};
```

#### Error Responses

| Status | Condition | Body |
|--------|-----------|------|
| `400` | Empty account_id | `{ "error": "account_id must not be empty", "code": "invalid_account_id" }` |

Individual event errors (bad date, invalid event_type, unparseable decimal) do not cause a 400. They are reported per-event in the `errors` array of the 200 response.

#### Example

**Request:**

```bash
curl -X POST http://127.0.0.1:7433/api/investments/import \
  -H "Content-Type: application/json" \
  -d '{
    "account_id": "acc_t212_isa",
    "events": [
      {
        "account_id": "acc_t212_isa",
        "event_type": "buy",
        "symbol": "AAPL",
        "date": "2026-04-10T14:30:00",
        "quantity": "10.0000",
        "price_per_share": "185.20",
        "fee": "0.00",
        "currency": "USD",
        "notes": null
      },
      {
        "account_id": "acc_t212_isa",
        "event_type": "buy",
        "symbol": "VUSA",
        "date": "2026-05-01T09:00:00",
        "quantity": "5.0000",
        "price_per_share": "76.32",
        "fee": null,
        "currency": "GBP",
        "notes": "Monthly ISA contribution"
      },
      {
        "account_id": "acc_t212_isa",
        "event_type": "invalid_type",
        "symbol": "TSLA",
        "date": "2026-05-10T10:00:00",
        "quantity": "2.0000",
        "price_per_share": "180.00",
        "fee": null,
        "currency": "USD",
        "notes": null
      }
    ]
  }'
```

**Response:**

```json
{
  "total": 3,
  "inserted": 2,
  "duplicates": 0,
  "errors": [
    {
      "index": 2,
      "reason": "invalid event_type: invalid_type"
    }
  ]
}
```

---

## Frontend Integration Guide

### TypeScript Imports

All types are available from `frontend/src/bindings/`:

```typescript
import type { IngestionPreview } from "../bindings/IngestionPreview";
import type { IngestionStatus } from "../bindings/IngestionStatus";
import type { IngestionMetadata } from "../bindings/IngestionMetadata";
import type { TransactionIngestionResult } from "../bindings/TransactionIngestionResult";
import type { HoldingsIngestionResult } from "../bindings/HoldingsIngestionResult";
import type { InvestmentIngestionResult } from "../bindings/InvestmentIngestionResult";
import type { TransactionPreviewRow } from "../bindings/TransactionPreviewRow";
import type { HoldingPreview } from "../bindings/HoldingPreview";
import type { InvestmentPreviewRow } from "../bindings/InvestmentPreviewRow";
import type { ClarificationRequest } from "../bindings/ClarificationRequest";
import type { ImportPayload } from "../bindings/ImportPayload";
import type { HoldingsImportPayload } from "../bindings/HoldingsImportPayload";
import type { InvestmentsImportPayload } from "../bindings/InvestmentsImportPayload";
import type { ImportResult } from "../bindings/ImportResult";
import type { InvestmentImportResult } from "../bindings/InvestmentImportResult";
```

### Flow Implementation

```typescript
// 1. Upload files to Stage 1
async function parseDocuments(
  files: File[],
  accountId?: string,
  hints?: { institution?: string; expected_data?: string[]; date_format?: string }
): Promise<IngestionPreview> {
  const formData = new FormData();
  files.forEach((file) => formData.append("files[]", file));
  if (accountId) formData.append("account_id", accountId);
  if (hints) formData.append("hints", JSON.stringify(hints));

  const res = await fetch("/api/parse", { method: "POST", body: formData });
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

// 2. After user reviews the preview and clicks confirm, commit each section
async function commitTransactions(payload: ImportPayload): Promise<ImportResult> {
  const res = await fetch("/api/import", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
  });
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

async function commitHoldings(payload: HoldingsImportPayload): Promise<{ inserted: number; updated: number; total: number }> {
  const res = await fetch("/api/holdings/import", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
  });
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

async function commitInvestments(payload: InvestmentsImportPayload): Promise<InvestmentImportResult> {
  const res = await fetch("/api/investments/import", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
  });
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}
```

### Preview Rendering Logic

The frontend should render a preview section for each data type that has a non-zero count. Hide sections with no data.

```typescript
function shouldShowSection(preview: IngestionPreview): {
  transactions: boolean;
  holdings: boolean;
  investments: boolean;
} {
  return {
    transactions: preview.transactions.count > 0,
    holdings: preview.holdings.count > 0,
    investments: preview.investments.count > 0,
  };
}
```

For each section, the `rows` array contains every row (new, duplicate, and error). Use `status` to style rows differently:

| Status | Meaning | Suggested Style |
|--------|---------|-----------------|
| `"new"` | Will be inserted on confirm | Green highlight or default |
| `"duplicate"` | Already in the database, will be skipped | Grey/muted, strikethrough |
| `"error"` | Failed to parse, will be skipped | Red highlight, show `error_reason` |
| `"modify"` | Exists but values differ (holdings only) | Yellow/amber highlight, show old vs new value |

The `payload` field in each section contains only the rows that should be committed (new rows, or new + modified for holdings). The frontend sends the payload as-is; no filtering is needed.

### Handling `needs_clarification`

When `status === "needs_clarification"`:

1. Display each `ClarificationRequest` to the user.
2. Show `suggestions` as selectable options (buttons, chips, or a dropdown).
3. Allow the user to type a custom answer if none of the suggestions apply.
4. Retry the parse request with the user's answers as hints.

```typescript
if (preview.status === "needs_clarification") {
  // Show clarification UI
  const answers = await showClarificationDialog(preview.clarifications_needed);
  // Retry with hints
  const retryPreview = await parseDocuments(files, accountId, {
    institution: answers.institution,
    ...otherHints,
  });
}
```

### Error Handling

Stage 2 endpoints use partial-success semantics. A 200 response does not mean all rows succeeded. Always check the error arrays:

```typescript
const result = await commitTransactions(payload);
if (result.errors.length > 0) {
  // Show which rows failed and why
  result.errors.forEach(({ index, reason }) => {
    console.warn(`Transaction at index ${index} failed: ${reason}`);
  });
}
// Show summary: inserted ${result.rows_inserted}, skipped ${result.rows_duplicate} duplicates
```

### Current Phase Status

The `POST /api/parse` endpoint is currently a stub (Phase 0). It accepts files and validates the request but returns empty results (all counts at 0, all arrays empty, no payloads). This allows frontend development to proceed against the real response shape before the parsing logic is implemented.

The Stage 2 endpoints (`POST /api/import`, `POST /api/holdings/import`, `POST /api/investments/import`) are fully functional.
