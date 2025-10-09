use axum::{
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Json, Response},
};
use serde_json::json;
use std::net::{IpAddr, SocketAddr};
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

/// 判断是否为内网IP地址
fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => {
            let octets = ipv4.octets();
            // 10.0.0.0/8
            octets[0] == 10
                // 172.16.0.0/12
                || (octets[0] == 172 && octets[1] >= 16 && octets[1] <= 31)
                // 192.168.0.0/16
                || (octets[0] == 192 && octets[1] == 168)
                // 127.0.0.0/8 (loopback)
                || octets[0] == 127
        }
        IpAddr::V6(ipv6) => {
            // IPv6 loopback (::1)
            ipv6.is_loopback()
                // IPv6 unique local addresses (fc00::/7)
                || (ipv6.segments()[0] & 0xfe00) == 0xfc00
        }
    }
}

/// 提取客户端真实IP地址
/// 优先级：非内网的连接地址 > X-Real-IP > X-Forwarded-For > 连接地址
fn extract_client_ip(req: &Request, addr: &SocketAddr) -> String {
    let conn_ip = addr.ip();

    // 如果连接地址不是内网IP，直接使用它
    if !is_private_ip(&conn_ip) {
        return conn_ip.to_string();
    }

    // 连接地址是内网IP，尝试从 X-Real-IP 获取
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
    conn_ip.to_string()
}
