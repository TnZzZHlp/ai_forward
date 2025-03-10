use crate::{db::DB, CACHE};
use colored::Colorize;
use salvo::{handler, Depot, FlowCtrl, Request, Response};
use serde_json::Value;
use std::{sync::Arc, time::Instant};

#[handler]
pub async fn log(req: &mut Request, depot: &mut Depot, res: &mut Response, ctrl: &mut FlowCtrl) {
    // 记录请求的开始时间
    let now = Instant::now();
    ctrl.call_next(req, depot, res).await;
    let duration = now.elapsed();

    let ip = get_ip(req).await;

    let hit = depot.get::<&str>("hit_cache").unwrap_or(&"none");

    match *hit {
        "memory" => {
            tracing::info!(
                "IP: {}, Hit Cache: {}, Processing Time: {}",
                ip.green(),
                "memory".green(),
                format_duration(duration).green(),
            );
        }
        "db" => {
            tracing::info!(
                "IP: {}, Hit Cache: {}, Processing Time: {}",
                ip.green(),
                "db".green(),
                format_duration(duration).green(),
            );
        }
        _ => {
            if let (Ok(model), Ok(provider), Some(status_code)) = (
                depot.get::<String>("model"),
                depot.get::<String>("provider"),
                res.status_code,
            ) {
                tracing::info!(
                    "IP: {}, Status: {}, Model: {}, Provider: {}, Processing Time: {}",
                    ip.green(),
                    color_status(status_code.as_u16()).green(),
                    model.green(),
                    provider.green(),
                    format_duration(duration).green(),
                );
            }
        }
    }
}

fn color_status(status_code: u16) -> String {
    if status_code == 200 {
        format!("{}", status_code).green().to_string()
    } else {
        format!("{}", status_code).red().to_string()
    }
}

fn format_duration(duration: std::time::Duration) -> String {
    let secs = duration.as_secs();
    let millis = duration.subsec_millis();
    let micros = duration.subsec_micros() % 1000;
    let nanos = duration.subsec_nanos() % 1000;

    if secs > 0 {
        if millis > 0 {
            format!("{}.{:03}s", secs, millis)
        } else {
            format!("{}s", secs)
        }
    } else if millis > 0 {
        if micros > 0 {
            format!("{}.{:03}ms", millis, micros)
        } else {
            format!("{}ms", millis)
        }
    } else if micros > 0 {
        if nanos > 0 {
            format!("{}.{:03}µs", micros, nanos)
        } else {
            format!("{}µs", micros)
        }
    } else {
        format!("{}ns", nanos)
    }
}

pub async fn get_ip(req: &Request) -> String {
    if let Some(ip) = req.headers().get("CF-Connecting-IP") {
        return ip.to_str().unwrap().to_string();
    }

    if let Some(ip) = req.headers().get("X-Real-IP") {
        return ip.to_str().unwrap().to_string();
    }

    if let Some(ip) = req.headers().get("X-Forwarded-For") {
        return ip.to_str().unwrap().split(',').next().unwrap().to_string();
    }

    req.remote_addr()
        .clone()
        .into_std()
        .unwrap()
        .ip()
        .to_string()
}

pub async fn record(messages: Value, response: String) {
    let messages = Arc::new(messages);
    let response = Arc::new(response);

    let cache = CACHE.get().unwrap();
    cache.insert(messages.clone(), response.clone()).await;

    // 保存到数据库
    DB.get()
        .unwrap()
        .save_to_db(messages.clone(), response.clone())
        .await;
}
