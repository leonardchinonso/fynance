//! Business income sync (plan 30 section 8): fetch monthly sales periods from
//! the shop app's endpoint and upsert them as `shop_sync` income records.
//!
//! The endpoint widens a requested range to whole periods, so the periods
//! returned may extend past `from`/`to`; they are mapped as returned, keyed by
//! `start..end`. Neither the token nor the response body is ever logged at INFO.

use std::collections::HashSet;
use std::time::Duration;

use rust_decimal::Decimal;
use serde::Deserialize;

use crate::income::line_types::IncomeLineType;
use crate::income::validate::{parse_iso_date, validate_currency, validate_record};
use crate::model::{
    IncomeBasis, IncomeLineItem, IncomeLineItemInput, IncomeOrigin, IncomeRecordInsert,
    IncomeRecordKind, IncomeSource, IncomeSyncLateRefund, IncomeSyncPeriod, IncomeSyncResult,
    IncomeSyncStatus,
};
use crate::server::error::AppError;
use crate::storage::db::Db;

const URL_VAR: &str = "FYNANCE_SHOP_SYNC_URL";
const TOKEN_VAR: &str = "FYNANCE_SHOP_SYNC_TOKEN";
const ORIGIN: &str = "shop_sync";

/// Where to fetch from, read from the environment on each sync.
pub struct SyncConfig {
    url: String,
    token: String,
}

impl SyncConfig {
    pub fn from_env() -> Result<Self, AppError> {
        let read = |name: &str| {
            std::env::var(name)
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        match (read(URL_VAR), read(TOKEN_VAR)) {
            (Some(url), Some(token)) => Ok(Self { url, token }),
            _ => Err(AppError::bad_request(
                format!("set {URL_VAR} and {TOKEN_VAR} to enable the shop sync"),
                "sync_not_configured",
            )),
        }
    }
}

/// The endpoint's response body (plan 30 section 8).
#[derive(Debug, Deserialize)]
pub struct ShopSyncResponse {
    #[serde(default)]
    pub basis: Option<String>,
    pub currency: String,
    pub periods: Vec<ShopPeriod>,
    #[serde(default)]
    pub late_refunds: Vec<ShopLateRefund>,
}

#[derive(Debug, Deserialize)]
pub struct ShopPeriod {
    pub start: String,
    pub end: String,
    #[serde(default)]
    pub sales_pence: i64,
    #[serde(default)]
    pub refunds_pence: i64,
    #[serde(default)]
    pub platform_fees_pence: i64,
    #[serde(default)]
    pub platform_postage_pence: i64,
    #[serde(default)]
    pub costing_materials_pence: i64,
    #[serde(default)]
    pub costing_packaging_pence: i64,
    #[serde(default)]
    pub costing_postage_pence: i64,
    #[serde(default)]
    pub costing_other_pence: i64,
    #[serde(default)]
    pub orders_missing_fee: u32,
    #[serde(default)]
    pub orders_missing_costs: u32,
}

#[derive(Debug, Deserialize)]
pub struct ShopLateRefund {
    pub order_month: String,
    pub refund_date: String,
    pub amount_pence: i64,
}

fn upstream(message: impl Into<String>) -> AppError {
    AppError::bad_gateway(message, "sync_upstream_error")
}

/// GET `<url>?from=&to=` with the bearer token. A non-200 answer is a 502
/// naming the status code only.
pub async fn fetch(
    config: &SyncConfig,
    from: &str,
    to: &str,
) -> Result<ShopSyncResponse, AppError> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        // A redirect could carry the token to another host.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("failed to build HTTP client: {e}")))?;
    let response = client
        .get(&config.url)
        .query(&[("from", from), ("to", to)])
        .bearer_auth(&config.token)
        .send()
        .await
        .map_err(|e| {
            // The error text can embed the URL, so only its kind is reported.
            if e.is_timeout() {
                AppError::gateway_timeout("the shop sync endpoint timed out", "sync_upstream_error")
            } else {
                upstream("the shop sync endpoint could not be reached")
            }
        })?;
    let status = response.status();
    if status != reqwest::StatusCode::OK {
        return Err(upstream(format!(
            "the shop sync endpoint answered with status {}",
            status.as_u16()
        )));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|_| upstream("the shop sync response could not be read"))?;
    let parsed: ShopSyncResponse = serde_json::from_slice(&bytes)
        .map_err(|_| upstream("the shop sync response is not in the expected shape"))?;
    tracing::info!(
        periods = parsed.periods.len(),
        late_refunds = parsed.late_refunds.len(),
        "shop sync fetched"
    );
    Ok(parsed)
}

fn pounds(pence: i64, what: &str) -> Result<Decimal, AppError> {
    if pence < 0 {
        return Err(upstream(format!("{what} is negative")));
    }
    Ok(Decimal::new(pence, 2))
}

/// Line items for one period: zero amounts omitted, stable order.
fn line_items(p: &ShopPeriod) -> Result<Vec<IncomeLineItemInput>, AppError> {
    let table = [
        (IncomeLineType::Sales, p.sales_pence),
        (IncomeLineType::Refunds, p.refunds_pence),
        (IncomeLineType::PlatformFees, p.platform_fees_pence),
        (IncomeLineType::PlatformPostage, p.platform_postage_pence),
        (IncomeLineType::CostingMaterials, p.costing_materials_pence),
        (IncomeLineType::CostingPackaging, p.costing_packaging_pence),
        (IncomeLineType::CostingPostage, p.costing_postage_pence),
        (IncomeLineType::CostingOther, p.costing_other_pence),
    ];
    let mut items = Vec::new();
    for (line_type, pence) in table {
        let amount = pounds(pence, line_type.as_str())?;
        if amount.is_zero() {
            continue;
        }
        items.push(IncomeLineItemInput {
            line_type,
            amount,
            currency: None,
            basis: IncomeBasis::Period,
            label: None,
            investment_event_id: None,
        });
    }
    Ok(items)
}

/// True when the stored items hold exactly the incoming figures.
fn same_items(stored: &[IncomeLineItem], incoming: &[IncomeLineItemInput]) -> bool {
    let mut a: Vec<_> = stored
        .iter()
        .map(|i| (i.line_type.as_str(), i.amount.normalize()))
        .collect();
    let mut b: Vec<_> = incoming
        .iter()
        .map(|i| (i.line_type.as_str(), i.amount.normalize()))
        .collect();
    a.sort();
    b.sort();
    a == b && stored.iter().all(|i| i.basis == IncomeBasis::Period)
}

/// The text of a 400, for folding an upstream-data problem into a 502.
fn message(e: &AppError) -> String {
    match e {
        AppError::BadRequest { message, .. } => message.clone(),
        _ => "invalid".to_string(),
    }
}

/// Map the response onto the source's records. With `dry_run` nothing is
/// written; otherwise the caller runs this inside one SQLite transaction.
/// Only records with `origin = shop_sync` are ever looked up or replaced.
pub fn apply(
    db: &Db,
    source: &IncomeSource,
    response: &ShopSyncResponse,
    dry_run: bool,
) -> Result<IncomeSyncResult, AppError> {
    validate_currency(db, &response.currency).map_err(|_| {
        upstream(format!(
            "the shop sync currency {:?} is not a known currency",
            response.currency
        ))
    })?;
    let mut result = IncomeSyncResult {
        dry_run,
        created: 0,
        updated: 0,
        unchanged: 0,
        periods: Vec::new(),
        late_refunds: Vec::new(),
        warnings: Vec::new(),
    };
    if let Some(basis) = response.basis.as_deref().filter(|b| *b != "order_date") {
        result.warnings.push(format!(
            "the endpoint reports basis {basis:?}, not order_date"
        ));
    }
    let mut seen = HashSet::new();
    for period in &response.periods {
        let external_ref = format!("{}..{}", period.start, period.end);
        if !seen.insert(external_ref.clone()) {
            return Err(upstream(format!("period {external_ref} appears twice")));
        }
        parse_iso_date(&period.start, "period start").map_err(|e| upstream(message(&e)))?;
        parse_iso_date(&period.end, "period end").map_err(|e| upstream(message(&e)))?;
        let items = line_items(period)?;
        let insert = IncomeRecordInsert {
            source_id: source.id.clone(),
            kind: IncomeRecordKind::Period,
            period_start: period.start.clone(),
            period_end: period.end.clone(),
            pay_date: None,
            currency: response.currency.clone(),
            transaction_ids: Vec::new(),
            document_ids: Vec::new(),
            origin: IncomeOrigin::ShopSync,
            external_ref: Some(external_ref.clone()),
            notes: None,
            line_items: items.clone(),
        };
        let tax_year = validate_record(db, source, &insert, true)
            .map_err(|e| upstream(format!("period {external_ref} rejected: {}", message(&e))))?;
        let status = match db.find_income_record_by_ref(&source.id, ORIGIN, &external_ref)? {
            None => {
                if !dry_run {
                    db.insert_income_record_inner(&insert, &tax_year)?;
                }
                result.created += 1;
                IncomeSyncStatus::Created
            }
            Some(existing) => {
                if existing.currency != response.currency {
                    return Err(AppError::conflict(
                        format!("period {external_ref} is stored in {}", existing.currency),
                        "sync_currency_changed",
                    ));
                }
                if same_items(&existing.line_items, &items) {
                    result.unchanged += 1;
                    IncomeSyncStatus::Unchanged
                } else {
                    if !dry_run {
                        db.replace_income_line_items(&existing.id, &existing.currency, &items)?;
                    }
                    result.updated += 1;
                    IncomeSyncStatus::Updated
                }
            }
        };
        if period.orders_missing_fee > 0 {
            result.warnings.push(format!(
                "{external_ref}: {} orders have no platform fee recorded",
                period.orders_missing_fee
            ));
        }
        if period.orders_missing_costs > 0 {
            result.warnings.push(format!(
                "{external_ref}: {} orders have no costing recorded",
                period.orders_missing_costs
            ));
        }
        result.periods.push(IncomeSyncPeriod {
            external_ref,
            status,
            line_items: items,
        });
    }
    for refund in &response.late_refunds {
        result.late_refunds.push(IncomeSyncLateRefund {
            order_month: refund.order_month.clone(),
            refund_date: refund.refund_date.clone(),
            amount: pounds(refund.amount_pence, "a late refund")?,
        });
    }
    Ok(result)
}
