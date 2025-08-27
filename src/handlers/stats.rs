use axum::{
    extract::State,
    http::StatusCode,
    response::{Json, IntoResponse},
};
use serde_json::json;

use crate::services::ai::AIService;
use crate::state::AppState;

pub async fn get_stats(State(app_state): State<AppState>) -> impl IntoResponse {
    let ai_service = AIService::new(app_state.clone());

    match ai_service.get_usage_stats().await {
        Ok(stats) => {
            (StatusCode::OK, Json(json!({
                "status": "ok",
                "stats": stats
            }))).into_response()
        }
        Err(e) => {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({
                "error": {
                    "message": e.to_string(),
                    "type": "service_error"
                }
            }))).into_response()
        }
    }
}

pub async fn reset_stats(State(app_state): State<AppState>) -> impl IntoResponse {
    // 重置统计信息
    {
        let provider_usage = app_state.provider_usage.write().await;
        provider_usage.clear();
    }

    {
        let key_usage = app_state.key_usage.write().await;
        key_usage.clear();
    }

    Json(json!({
        "status": "ok",
        "message": "Statistics reset successfully"
    }))
}
