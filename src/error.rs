use crate::config::ConfigError;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, error_msg) = match &self {
            AppError::Database(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Database error occurred"),
            AppError::Http(_) => (StatusCode::BAD_GATEWAY, "Upstream service error"),
            AppError::Json(_) => (StatusCode::BAD_REQUEST, "Invalid JSON format"),
            AppError::Config(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Configuration error"),
            AppError::Validation(_) => (StatusCode::BAD_REQUEST, "Validation failed"),
            AppError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error"),
        };

        let error_response = json!({
            "error": {
                "message": error_msg,
                "type": format!("{:?}", self).split('(').next().unwrap_or("Unknown"),
            }
        });

        (status, Json(error_response)).into_response()
    }
}

pub type AppResult<T> = Result<T, AppError>;
