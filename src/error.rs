use crate::config::ConfigError;
use axum::{
    http::StatusCode,
    response::{Json, IntoResponse, Response},
};
use serde_json::json;
use std::fmt;

#[derive(Debug)]
pub enum AppError {
    Database(sqlx::Error),
    Http(reqwest::Error),
    Json(serde_json::Error),
    Config(ConfigError),
    Validation(String),
    Internal(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Database(e) => write!(f, "Database error: {}", e),
            AppError::Http(e) => write!(f, "HTTP error: {}", e),
            AppError::Json(e) => write!(f, "JSON error: {}", e),
            AppError::Config(e) => write!(f, "Configuration error: {}", e),
            AppError::Validation(msg) => write!(f, "Validation error: {}", msg),
            AppError::Internal(msg) => write!(f, "Internal error: {}", msg),
        }
    }
}

impl std::error::Error for AppError {}

impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        AppError::Database(err)
    }
}

impl From<reqwest::Error> for AppError {
    fn from(err: reqwest::Error) -> Self {
        AppError::Http(err)
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        AppError::Json(err)
    }
}

impl From<ConfigError> for AppError {
    fn from(err: ConfigError) -> Self {
        AppError::Config(err)
    }
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
