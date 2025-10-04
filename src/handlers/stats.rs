use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use serde_json::json;

use crate::state::AppState;
use crate::{error::AppResult, services::ai::AIService};

pub async fn get_stats(State(app_state): State<AppState>) -> impl IntoResponse {
    let ai_service = AIService::new(app_state.clone());

    match ai_service.get_usage_stats().await {
        Ok(stats) => (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "stats": stats
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": {
                    "message": e.to_string(),
                    "type": "service_error"
                }
            })),
        )
            .into_response(),
    }
}

pub async fn reset_stats(State(app_state): State<AppState>) -> AppResult<Response> {
    // 重置统计信息
    {
        let provider_usage = app_state.provider_usage.write().await;
        provider_usage.clear();
    }

    {
        let key_usage = app_state.key_usage.write().await;
        key_usage.clear();
    }

    // 重新读取配置文件
    {
        app_state.reload_config().await?;
    }

    Ok((
        StatusCode::OK,
        Json(json!({"message": "stats reset and config reloaded"})),
    )
        .into_response())
}
