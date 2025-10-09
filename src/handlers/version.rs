use axum::{response::Json, http::StatusCode};
use serde_json::json;

/// 获取版本信息和编译时间
pub async fn get_version() -> (StatusCode, Json<serde_json::Value>) {
    // 获取编译时间
    let build_time = env!("BUILD_TIME");
    let version = env!("CARGO_PKG_VERSION");
    
    (
        StatusCode::OK,
        Json(json!({
            "version": version,
            "build_time": build_time,
        })),
    )
}
