use axum::{
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Json, Response},
};
use serde_json::json;
use std::net::SocketAddr;
use tracing::warn;

use crate::state::AppState;

pub async fn auth_handler(
    State(app_state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
    // 优先从 X-Real-IP 请求头获取真实IP，否则使用连接地址
    let client_ip = req
        .headers()
        .get("x-real-ip")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| addr.ip().to_string());

    // 检查IP是否已被封禁
    if app_state.ip_ban_manager.is_banned(&client_ip) {
        warn!("Blocked banned IP: {}", client_ip);
        let error_response = Json(json!({
            "error": {
                "message": "Your IP has been permanently banned due to multiple failed authentication attempts",
                "type": "ip_banned"
            }
        }));
        return (StatusCode::FORBIDDEN, error_response).into_response();
    }

    let auth_header = req.headers().get("authorization");

    if let Some(auth_header) = auth_header {
        if let Ok(auth_str) = auth_header.to_str() {
            if let Some(token) = auth_str.strip_prefix("Bearer ") {
                let config = app_state.config.read().await;
                if token == config.auth {
                    // 认证成功，重置该IP的失败次数
                    app_state.ip_ban_manager.reset_failures(&client_ip);
                    return next.run(req).await;
                }
            }
        }
    }

    // 认证失败，记录失败次数
    app_state.ip_ban_manager.record_failure(&client_ip);
    warn!(
        "Unauthorized request from IP: {}, failure count: {}",
        client_ip,
        app_state.ip_ban_manager.get_failure_count(&client_ip)
    );

    let error_response = Json(json!({
        "error": {
            "message": "Invalid authorization token",
            "type": "auth_error"
        }
    }));

    (StatusCode::UNAUTHORIZED, error_response).into_response()
}
