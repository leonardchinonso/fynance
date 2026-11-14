//! Income routes (plan 30). Handlers are filled in after the contract commit.

use axum::Json;
use serde_json::Value;

use crate::server::error::AppError;

pub async fn list_line_types() -> Result<Json<Value>, AppError> {
    Err(AppError::Internal(anyhow::anyhow!("not implemented")))
}

pub async fn vest_candidates() -> Result<Json<Value>, AppError> {
    Err(AppError::Internal(anyhow::anyhow!("not implemented")))
}

pub async fn list_sources() -> Result<Json<Value>, AppError> {
    Err(AppError::Internal(anyhow::anyhow!("not implemented")))
}

pub async fn create_source() -> Result<Json<Value>, AppError> {
    Err(AppError::Internal(anyhow::anyhow!("not implemented")))
}

pub async fn update_source() -> Result<Json<Value>, AppError> {
    Err(AppError::Internal(anyhow::anyhow!("not implemented")))
}

pub async fn delete_source() -> Result<Json<Value>, AppError> {
    Err(AppError::Internal(anyhow::anyhow!("not implemented")))
}

pub async fn create_year_end_adjustment() -> Result<Json<Value>, AppError> {
    Err(AppError::Internal(anyhow::anyhow!("not implemented")))
}

pub async fn list_records() -> Result<Json<Value>, AppError> {
    Err(AppError::Internal(anyhow::anyhow!("not implemented")))
}

pub async fn create_record() -> Result<Json<Value>, AppError> {
    Err(AppError::Internal(anyhow::anyhow!("not implemented")))
}

pub async fn get_record() -> Result<Json<Value>, AppError> {
    Err(AppError::Internal(anyhow::anyhow!("not implemented")))
}

pub async fn update_record() -> Result<Json<Value>, AppError> {
    Err(AppError::Internal(anyhow::anyhow!("not implemented")))
}

pub async fn delete_record() -> Result<Json<Value>, AppError> {
    Err(AppError::Internal(anyhow::anyhow!("not implemented")))
}

pub async fn list_checks() -> Result<Json<Value>, AppError> {
    Err(AppError::Internal(anyhow::anyhow!("not implemented")))
}

pub async fn create_check() -> Result<Json<Value>, AppError> {
    Err(AppError::Internal(anyhow::anyhow!("not implemented")))
}

pub async fn update_check() -> Result<Json<Value>, AppError> {
    Err(AppError::Internal(anyhow::anyhow!("not implemented")))
}

pub async fn delete_check() -> Result<Json<Value>, AppError> {
    Err(AppError::Internal(anyhow::anyhow!("not implemented")))
}

pub async fn get_tax_year_summary() -> Result<Json<Value>, AppError> {
    Err(AppError::Internal(anyhow::anyhow!("not implemented")))
}
