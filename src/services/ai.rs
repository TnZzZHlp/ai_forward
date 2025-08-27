use axum::{
    body::Body,
    http::{HeaderMap, StatusCode},
    response::Response,
};
use serde_json::{json, Value};
use tracing::{debug, error};

use crate::config::Provider;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

pub struct AIService {
    state: AppState,
}

impl AIService {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    async fn select_api_key(&self, provider: &Provider) -> AppResult<String> {
        if provider.keys.is_empty() {
            return Err(AppError::Validation(format!(
                "No API keys configured for provider '{}'",
                provider.name
            )));
        }

        // 简单的轮询策略，可以后续改进为更智能的负载均衡
        let key_usage = self.state.key_usage.read().await;
        let mut min_usage = u64::MAX;
        let mut selected_key = &provider.keys[0];

        for key in &provider.keys {
            let usage = key_usage.get(key).map(|v| *v).unwrap_or(0);
            if usage < min_usage {
                min_usage = usage;
                selected_key = key;
            }
        }

        Ok(selected_key.clone())
    }

    async fn update_usage_stats(&self, provider: &Provider, api_key: &str) {
        // 更新提供者使用统计
        {
            let provider_usage = self.state.provider_usage.write().await;
            *provider_usage.entry(provider.name.clone()).or_insert(0) += 1;
        }

        // 更新密钥使用统计
        {
            let key_usage = self.state.key_usage.write().await;
            *key_usage.entry(api_key.to_string()).or_insert(0) += 1;
        }

        debug!(
            "Updated usage stats for provider '{}' and key",
            provider.name
        );
    }

    pub async fn forward_request_with_model_replacement(
        &self,
        mut payload: Value,
        model: String,
        _headers: HeaderMap,
    ) -> AppResult<Response> {
        // 查找提供者
        let provider = self
            .state
            .get_provider_by_model(&model)
            .ok_or_else(|| AppError::Validation(format!("Model '{}' not found", model)))?;

        // 获取真实模型名称
        let real_model = self.state.get_model_mapping(&model).ok_or_else(|| {
            AppError::Validation(format!("Model mapping not found for '{}'", model))
        })?;

        // 只替换payload中的model字段
        payload["model"] = Value::String(real_model);

        // 选择API密钥
        let api_key = self.select_api_key(provider).await?;

        // 直接转发请求并返回流式响应
        let response = self
            .state
            .http_client
            .post(&provider.url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        // 更新使用统计
        self.update_usage_stats(provider, &api_key).await;

        // 检查响应状态
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            error!("API request failed: {} - {}", status, error_text);
            return Err(AppError::Internal(format!(
                "API request failed: {}",
                status
            )));
        }

        // 获取响应头
        let mut response_headers = HeaderMap::new();
        for (key, value) in response.headers() {
            if let Ok(header_name) = axum::http::HeaderName::from_bytes(key.as_str().as_bytes()) {
                response_headers.insert(header_name, value.clone());
            }
        }

        // 获取响应体作为字节流
        let response_bytes = response.bytes().await?;
        let body = Body::from(response_bytes);

        // 构建响应
        let mut axum_response = Response::builder().status(StatusCode::OK);

        // 添加响应头
        if let Some(headers) = axum_response.headers_mut() {
            *headers = response_headers;
        }

        let final_response = axum_response
            .body(body)
            .map_err(|e| AppError::Internal(format!("Failed to build response: {}", e)))?;

        Ok(final_response)
    }

    pub async fn get_usage_stats(&self) -> AppResult<Value> {
        let provider_usage = self.state.provider_usage.read().await;

        Ok(json!({
            "provider_usage": provider_usage.iter().map(|entry| {
                json!({
                    "provider": entry.key(),
                    "usage": *entry.value()
                })
            }).collect::<Vec<_>>()
        }))
    }
}
