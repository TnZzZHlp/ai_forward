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
    // 获取客户端真实IP，优先级：X-Real-IP > X-Forwarded-For > 连接地址
    let client_ip = extract_client_ip(&req, &addr);

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

/// 提取客户端真实IP地址
/// 优先级：X-Real-IP > X-Forwarded-For > 连接地址
fn extract_client_ip(req: &Request, addr: &SocketAddr) -> String {
    // 首先尝试从 X-Real-IP 获取
    if let Some(real_ip) = req.headers().get("x-real-ip").and_then(|h| h.to_str().ok()) {
        return real_ip.to_string();
    }

    // 然后尝试从 X-Forwarded-For 获取
    if let Some(forwarded_for) = req
        .headers()
        .get("x-forwarded-for")
        .and_then(|h| h.to_str().ok())
    {
        // X-Forwarded-For 可能包含多个IP，取第一个
        if let Some(first_ip) = forwarded_for.split(',').next() {
            return first_ip.trim().to_string();
        }
    }

    // 最后使用连接地址
    addr.ip().to_string()
}
