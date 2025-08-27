use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json},
    Json as AxumJson,
};
use serde_json::{json, Value};

use crate::services::ai::AIService;
use crate::state::AppState;

pub async fn chat_completions(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    AxumJson(payload): AxumJson<Value>,
) -> impl IntoResponse {
    let ai_service = AIService::new(app_state.clone());

    // 从JSON中提取model字段
    let model = match payload.get("model").and_then(|v| v.as_str()) {
        Some(model) => model.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": {
                        "message": "Missing or invalid model field",
                        "type": "validation_error"
                    }
                })),
            )
                .into_response();
        }
    };

    // 直接转发请求，只替换model字段
    match ai_service
        .forward_request_with_model_replacement(payload, model, headers)
        .await
    {
        Ok(response) => response,
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

pub async fn list_models(State(app_state): State<AppState>) -> impl IntoResponse {
    let models: Vec<Value> = app_state
        .config
        .providers
        .iter()
        .flat_map(|provider| &provider.models)
        .map(|model| {
            json!({
                "id": model.alias,
                "object": "model",
                "created": 0,
                "owned_by": "ai_forward"
            })
        })
        .collect();

    Json(json!({
        "object": "list",
        "data": models
    }))
}
