use salvo::{http::request, http::response, prelude::*};
use serde_json::json;
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
            res.stream(ai_res.bytes_stream());
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
            if res.status() == 401 {
                error!("提供者 {} 的密钥 {} 无效", provider.name, key);
            }

            // 判断状态
            if res.status() != 200 {
                let text = res.text().await.unwrap();
                error!("提供者 {} 返回了错误: {}", provider.name, text);
                Err(json!({"error": text, "provider": provider.name}))
            } else {
                Ok(res)
            }
        }
        Err(e) => Err(json!({"error": e.to_string(), "provider": provider.name})),
    };

    // 记录模型和提供者
    depot.insert("model", model.to_string());
    depot.insert("provider", provider.name.clone());

    resp
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
