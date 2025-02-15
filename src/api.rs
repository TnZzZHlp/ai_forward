use std::convert::Infallible;
use std::sync::Arc;

use eventsource_stream::Eventsource;
use futures::stream::StreamExt;
use salvo::sse;
use salvo::{http::request, http::response, prelude::*};
use serde_json::json;
use tokio::sync::Mutex;
use tracing::error;

use crate::config::Provider;
use crate::{CLIENT, CONFIG, KEY_USAGE_COUNT, PROVIDER_USAGE_COUNT};

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
    let payload = req.payload_with_max_size(1024 * 1024 * 10).await.unwrap();

    let mut json: serde_json::Value =
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

    // 调用AI
    match forward(&mut json, depot).await {
        Ok(ai_res) => {
            if json["stream"].as_bool().unwrap_or(false) {
                let original_stream = ai_res.bytes_stream();
                res.stream(original_stream);
                res.headers_mut()
                    .insert("Content-Type", "text/event-stream".parse().unwrap());
            } else {
                // 直接返回
                let original_stream = ai_res.bytes_stream();
                res.stream(original_stream);
                res.headers_mut()
                    .insert("Content-Type", "application/json".parse().unwrap());
            }
        }
        Err(e) => {
            res.stuff(StatusCode::INTERNAL_SERVER_ERROR, Json(e));
        }
    }
}

#[handler]
pub async fn no_think_completions(
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
    let payload = req.payload_with_max_size(1024 * 1024 * 10).await.unwrap();

    let mut json: serde_json::Value =
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

    // 调用AI
    match forward(&mut json, depot).await {
        Ok(ai_res) => {
            if json["stream"].as_bool().unwrap_or(false) {
                let thinked = Arc::new(Mutex::new(false));
                let buffer = Arc::new(Mutex::new(String::new()));

                let stream = ai_res.bytes_stream().eventsource().then(move |event| {
                    let thinked = thinked.clone();
                    let buffer = buffer.clone();
                    async move {
                        match event {
                            Ok(event) => {
                                if *thinked.lock().await {
                                    Ok::<SseEvent, Infallible>(SseEvent::default().text(event.data))
                                } else {
                                    let mut json = match serde_json::from_str::<serde_json::Value>(
                                        &event.data,
                                    ) {
                                        Ok(json) => json,
                                        Err(_) => {
                                            return Ok(SseEvent::default().text(event.data));
                                        }
                                    };

                                    let mut buffer = buffer.lock().await;

                                    // 写入缓冲区
                                    buffer.push_str(
                                        match json["choices"][0]["delta"]["content"].as_str() {
                                            Some(content) => content,
                                            None => {
                                                error!("解析内容出现错误: {}", json);
                                                return Ok(SseEvent::default().json(json).unwrap());
                                            }
                                        },
                                    );

                                    // 如果前3个字符不是<th，则认为该模型不支持思考
                                    if !buffer.starts_with("<th") && buffer.chars().count() > 3 {
                                        *thinked.lock().await = true;
                                        json["choices"][0]["delta"]["content"] =
                                            buffer.to_string().into();

                                        return Ok(SseEvent::default().json(json).unwrap());
                                    }

                                    // 如果有</think>，则认为已经思考完毕
                                    if buffer.contains("</think>\n\n") {
                                        *thinked.lock().await = true;
                                        json["choices"][0]["delta"]["content"] = buffer
                                            .split("</think>\n\n")
                                            .last()
                                            .unwrap()
                                            .to_string()
                                            .into();
                                    } else {
                                        json["choices"][0]["delta"]["content"] =
                                            String::new().into();
                                    }

                                    Ok(SseEvent::default().json(json).unwrap())
                                }
                            }
                            Err(e) => Ok(SseEvent::default().text(e.to_string())),
                        }
                    }
                });

                sse::stream(res, stream);
                res.headers_mut()
                    .insert("Content-Type", "text/event-stream".parse().unwrap());
            } else {
                // 直接返回
                let mut json = ai_res.json::<serde_json::Value>().await.unwrap();
                json["choices"][0]["message"]["content"] = json["choices"][0]["message"]["content"]
                    .as_str()
                    .unwrap()
                    .split("</think>\n\n")
                    .last()
                    .unwrap()
                    .to_string()
                    .into();
                res.render(Json(json));
                res.headers_mut()
                    .insert("Content-Type", "application/json".parse().unwrap());
            }
        }
        Err(e) => {
            res.stuff(StatusCode::INTERNAL_SERVER_ERROR, Json(e));
        }
    }
}

async fn forward(
    json: &mut serde_json::Value,
    depot: &mut Depot,
) -> Result<reqwest::Response, serde_json::Value> {
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

    Ok(resp)
}

async fn select_provider(providers: Vec<&Provider>) -> Result<&Provider, String> {
    // 找到PROVIDER_USAGE_COUNT中使用次数最少的提供者
    let count = PROVIDER_USAGE_COUNT.get().unwrap().read().unwrap();
    let provider = match providers
        .iter()
        .min_by_key(|x| count.get(&x.name).unwrap_or(&0))
    {
        Some(provider) => provider,
        None => {
            return Err("没有找到能处理该模型的提供者".to_string());
        }
    };

    drop(count);

    let mut count = PROVIDER_USAGE_COUNT.get().unwrap().write().unwrap();
    *count.entry(provider.name.clone()).or_insert(0) += 1;

    Ok(provider)
}

async fn select_key(provider: &Provider) -> Result<&str, String> {
    // 在Provider中找到KEY_USAGE_COUNT中使用次数最少的提供者
    let count = KEY_USAGE_COUNT.get().unwrap().read().unwrap();
    let key = match provider
        .keys
        .iter()
        .min_by_key(|x| count.get(*x).unwrap_or(&0))
    {
        Some(key) => key,
        None => {
            return Err(format!("提供者 {} 没有可用的密钥", provider.name));
        }
    };

    drop(count);

    let mut count = KEY_USAGE_COUNT.get().unwrap().write().unwrap();

    *count.entry(key.to_string()).or_insert(0) += 1;

    Ok(key)
}
