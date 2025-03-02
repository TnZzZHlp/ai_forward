use colored::Colorize;
use salvo::{handler, Depot, FlowCtrl, Request, Response};
use std::time::Instant;
use tokio::io::AsyncWriteExt;

use crate::CACHE;

#[handler]
pub async fn log(req: &mut Request, depot: &mut Depot, res: &mut Response, ctrl: &mut FlowCtrl) {
    let now = Instant::now();
    ctrl.call_next(req, depot, res).await;
    let duration = now.elapsed();

    let status = res.status_code.unwrap().as_u16();
    let model = match depot.get::<String>("model") {
        Ok(model) => model,
        Err(_) => "",
    };

    let provider = match depot.get::<String>("provider") {
        Ok(provider) => provider,
        Err(_) => "",
    };

    // 每50个请求保存到文件一次
    let hit_cache = depot.get::<bool>("hit_cache").unwrap_or(&false);
    if !*hit_cache {
        let cache = CACHE.get().unwrap();

        if cache.entry_count() % 50 == 0 {
            let mut file = tokio::fs::OpenOptions::new()
                .write(true)
                .append(true)
                .create(true)
                .open("cache")
                .await
                .unwrap();

            for (k, v) in cache.iter() {
                file.write_all(format!("{k}\n===\n{v}").as_bytes())
                    .await
                    .unwrap();
                // 添加分隔符
                file.write_all(b"\n+++\n").await.unwrap();
            }
        }
    }

    let ip = get_ip(req).await;

    tracing::info!(
        "IP: {}, Status: {}, Model: {}, Provider: {}, Processing Time: {}, Hit Cache: {}",
        ip.green(),
        if status == 200 {
            status.to_string().green()
        } else {
            status.to_string().red()
        },
        model.green(),
        provider.green(),
        format_duration(duration).green(),
        if *hit_cache {
            "true".green()
        } else {
            "false".red()
        }
    );
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
