use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Json, Response},
};
use serde_json::json;
use std::time::Instant;
use tracing::{info, warn};

use crate::state::AppState;

pub async fn auth_handler(State(app_state): State<AppState>, req: Request, next: Next) -> Response {
    let auth_header = req.headers().get("authorization");

    if let Some(auth_header) = auth_header {
        if let Ok(auth_str) = auth_header.to_str() {
            if let Some(token) = auth_str.strip_prefix("Bearer ") {
                if token == app_state.config.auth {
                    return next.run(req).await;
                }
            }
        }
    }

    warn!("Unauthorized request");

    let error_response = Json(json!({
        "error": {
            "message": "Invalid authorization token",
            "type": "auth_error"
        }
    }));

    (StatusCode::UNAUTHORIZED, error_response).into_response()
}

pub async fn logging_handler(req: Request, next: Next) -> Response {
    let start = Instant::now();
    let method = req.method().clone();
    let path = req.uri().path().to_string();

    info!("Request: {} {}", method, path);

    let response = next.run(req).await;

    let duration = start.elapsed();
    info!("Response: {} {} - {}ms", method, path, duration.as_millis());

    response
}

pub async fn response_time_handler(req: Request, next: Next) -> Response {
    let _start = Instant::now();
    let response = next.run(req).await;
    let _duration = _start.elapsed();

    response
}
