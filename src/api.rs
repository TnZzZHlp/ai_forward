use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use eventsource_stream::Eventsource;
use futures::stream::{self, StreamExt};
use salvo::sse;
use salvo::{http::request, http::response, prelude::*};
use serde_json::{json, Value};
use tokio::spawn;
use tokio::sync::Mutex;
use tracing::error;

use crate::config::Provider;
use crate::db::DB;
use crate::{logger, CACHE, CLIENT, CONFIG, KEY_USAGE_COUNT, PROVIDER_USAGE_COUNT};

#[handler]
pub async fn completions(
    res: &mut response::Response,
    req: &mut request::Request,
    depot: &mut Depot,
) {
    // 获取 Authorization
    match req.header::<&str>("Authorization") {
        Some(auth) => {
            if auth != format!("Bearer {}", CONFIG.get().unwrap().auth) {
                res.stuff(
                    StatusCode::UNAUTHORIZED,
                    Json(json!({ "error": "无效的 Authorization" })),
                );
                return;
            }
        }
        None => {
            res.stuff(
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "缺少 Authorization" })),
            );
            return;
        }
    }

    // 解析JSON
    // https://github.com/hyperium/hyper/issues/3111
    // 默认Payload大小为8KB，这里设置为10MB
    let payload = match req.payload_with_max_size(1024 * 1024 * 10).await {
        Ok(payload) => payload,
        Err(e) => {
            res.stuff(
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": e.to_string() })),
            );
            return;
        }
    };

    let mut req_json: serde_json::Value =
        match serde_json::from_str(std::str::from_utf8(payload).unwrap()) {
            Ok(json) => json,
            Err(e) => {
                res.stuff(
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": e.to_string() })),
                );
                return;
            }
        };

    // 查询缓存
    if reply_cache(&req_json, res, depot).await {
        return;
    }

    // 调用AI
    match forward(&mut req_json, depot).await {
        Ok((ai_res, _)) => {
            if req_json["stream"].as_bool().unwrap_or(false) {
                process_stream_reply(res, ai_res, req_json).await;
            } else {
                process_normal_reply(res, ai_res, req_json).await;
            }
        }
        Err(e) => {
            res.stuff(StatusCode::INTERNAL_SERVER_ERROR, Json(e));
        }
    }
}

async fn process_normal_reply(
    res: &mut response::Response,
    ai_res: reqwest::Response,
    req_json: Value,
) {
    // 直接返回
    let reply = match ai_res.json::<Value>().await {
        Ok(json) => json,
        Err(e) => {
            res.stuff(
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            );
            return;
        }
    };
    res.headers_mut()
        .insert("Content-Type", "application/json".parse().unwrap());

    // 记录缓存
    let messages = req_json["messages"].clone();
    let response = reply["choices"][0]["message"]["content"]
        .as_str()
        .unwrap()
        .to_string();
    spawn(logger::record(messages, response));

    res.render(Json(reply));
}

async fn process_stream_reply(
    res: &mut response::Response,
    ai_res: reqwest::Response,
    req_json: Value,
) {
    let buffer = Arc::new(Mutex::new(String::new()));

    // 缓存
    let (tx, mut rx) = tokio::sync::mpsc::channel::<bool>(1);
    let buffer_clone = Arc::clone(&buffer);
    tokio::spawn(async move {
        // 等待接收
        rx.recv().await;
        let buffer = buffer_clone.lock().await;
        logger::record(req_json["messages"].clone(), buffer.to_string()).await;
    });

    let stream = ai_res.bytes_stream().eventsource().then(move |event| {
        let buffer = buffer.clone();
        let tx = tx.clone();
        async move {
            match event {
                Ok(event) => {
                    let json = match serde_json::from_str::<Value>(&event.data) {
                        Ok(json) => json,
                        Err(_) => {
                            // 解析失败意味流结束，发送信号记录缓存
                            tx.send(true).await.unwrap();
                            return Ok::<_, Infallible>(SseEvent::default().text(event.data));
                        }
                    };

                    // 写入缓冲区
                    buffer.lock().await.push_str(
                        match json["choices"][0]["delta"]["content"].as_str() {
                            Some(content) => content,
                            None => {
                                return Ok(SseEvent::default().text(&event.data));
                            }
                        },
                    );

                    Ok(SseEvent::default().json(json).unwrap())
                }
                Err(e) => Ok(SseEvent::default().text(e.to_string())),
            }
        }
    });

    sse::stream(res, stream);
    res.headers_mut()
        .insert("Content-Type", "text/event-stream".parse().unwrap());
}

async fn reply_cache(req_json: &Value, res: &mut response::Response, depot: &mut Depot) -> bool {
    let mut reply = |cached: Arc<String>| {
        // 判断请求类型
        if req_json["stream"].as_bool().unwrap_or(false) {
            // 直接返回
            res.headers_mut()
                .insert("Content-Type", "text/event-stream".parse().unwrap());

            let event_stream = stream::iter(vec![
                Box::pin(async move {
                    Ok::<_, Infallible>(
                        SseEvent::default().text(
                            json!({
                                "choices": [
                                    {
                                        "delta": {
                                            "content": cached.as_str(),
                                            "role": "assistant"
                                        }
                                    }
                                ]
                            })
                            .to_string(),
                        ),
                    )
                })
                    as Pin<Box<dyn Future<Output = Result<SseEvent, Infallible>> + Send>>,
                Box::pin(async { Ok::<_, Infallible>(SseEvent::default().text("[DONE]")) })
                    as Pin<Box<dyn Future<Output = Result<SseEvent, Infallible>> + Send>>,
            ])
            .then(|future| future);

            sse::stream(res, event_stream);
        } else {
            // 直接返回
            res.headers_mut()
                .insert("Content-Type", "application/json".parse().unwrap());
            res.render(Json(json!({
                "choices": [
                    {
                        "message": {
                            "content": cached.as_str(),
                            "role": "assistant"
                        }
                    }
                ]
            })));
        }
    };

    // 查询缓存
    let cache = CACHE.get().unwrap();
    if let Some(cached) = cache.get(&req_json["messages"]) {
        depot.insert("hit_cache", "memory");
        // 直接返回
        reply(cached);
        return true;
    }

    // 查询数据库
    if let Some(ai_request) = DB.get().unwrap().get_from_db(&req_json["messages"]).await {
        depot.insert("hit_cache", "db");
        // 直接返回
        reply(ai_request.response.into());
        return true;
    }

    false
}

async fn forward(
    json: &mut serde_json::Value,
    depot: &mut Depot,
) -> Result<(reqwest::Response, String), serde_json::Value> {
    // 获取模型
    let model = match json["model"].as_str() {
        Some(model) => model,
        None => {
            return Err(json!({ "error": "缺少 model 字段" }));
        }
    };

    // 找到能处理该模型的提供者
    let providers = CONFIG
        .get()
        .unwrap()
        .providers
        .iter()
        .filter(|x| x.models.iter().any(|m| m.alias == model))
        .collect::<Vec<&Provider>>();

    // 找到PROVIDER_USAGE_COUNT中使用次数最少的提供者
    let provider = match select_provider(providers).await {
        Ok(provider) => provider,
        Err(e) => {
            return Err(json!({ "error": e }));
        }
    };

    // 在Provider中找到KEY_USAGE_COUNT中使用次数最少的提供者
    let key = match select_key(provider).await {
        Ok(key) => key,
        Err(e) => {
            return Err(json!({ "error": e }));
        }
    };

    let url = &provider.url;

    let model = &provider
        .models
        .iter()
        .find(|m| m.alias == model)
        .expect("查找模型出现问题")
        .model;

    // 替换源JSON中的模型
    json.as_object_mut().unwrap()["model"] = serde_json::Value::String(model.to_string());

    // 发送请求
    let resp = match CLIENT
        .get()
        .unwrap()
        .post(url)
        .header("Authorization", format!("Bearer {}", key))
        .header("Content-Type", "application/json")
        .json(&json)
        .send()
        .await
    {
        Ok(res) => {
            if res.status() == 401 || res.status() == 403 {
                error!("提供者 {} 的密钥 {} 无效", provider.name, key);
            }

            // 判断状态
            if res.status() != 200 {
                let text = res.text().await.unwrap();
                error!("提供者 {} 返回了错误: {}", provider.name, text);
                return Err(json!({"error": text, "provider": provider.name}));
            } else {
                res
            }
        }
        Err(e) => return Err(json!({"error": e.to_string(), "provider": provider.name})),
    };

    // 记录模型和提供者
    depot.insert("model", model.to_string());
    depot.insert("provider", provider.name.clone());

    Ok((resp, model.to_string()))
}

async fn select_provider(providers: Vec<&Provider>) -> Result<&Provider, String> {
    // 找到PROVIDER_USAGE_COUNT中使用次数最少的提供者
    let count = PROVIDER_USAGE_COUNT.get().unwrap();
    let provider = match providers.iter().min_by_key(|x| match count.get(&x.name) {
        Some(ref_val) => *ref_val,
        None => 0,
    }) {
        Some(provider) => provider,
        None => {
            return Err("没有找到能处理该模型的提供者".to_string());
        }
    };

    *count.entry(provider.name.clone()).or_insert(0) += 1;

    Ok(provider)
}

async fn select_key(provider: &Provider) -> Result<&str, String> {
    // 在Provider中找到KEY_USAGE_COUNT中使用次数最少的提供者
    let count = KEY_USAGE_COUNT.get().unwrap();
    let key = match provider.keys.iter().min_by_key(|x| match count.get(*x) {
        Some(ref_val) => *ref_val,
        None => 0,
    }) {
        Some(key) => key,
        None => {
            return Err(format!("提供者 {} 没有可用的密钥", provider.name));
        }
    };

    *count.entry(key.to_string()).or_insert(0) += 1;

    Ok(key)
}
