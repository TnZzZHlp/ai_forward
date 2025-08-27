use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::config::Config;
use crate::error::AppResult;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub http_client: reqwest::Client,
    pub provider_usage: Arc<RwLock<DashMap<String, u64>>>,
    pub key_usage: Arc<RwLock<DashMap<String, u64>>>,
}

impl AppState {
    pub async fn new(config: Config) -> AppResult<Self> {
        let http_client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(60))
            .build()?;

        Ok(Self {
            config: Arc::new(config),
            http_client,
            provider_usage: Arc::new(RwLock::new(DashMap::new())),
            key_usage: Arc::new(RwLock::new(DashMap::new())),
        })
    }

    pub fn get_provider_by_model(&self, model: &str) -> Option<&crate::config::Provider> {
        self.config
            .providers
            .iter()
            .find(|provider| provider.models.iter().any(|m| m.alias == model))
    }

    pub fn get_model_mapping(&self, alias: &str) -> Option<String> {
        for provider in &self.config.providers {
            if let Some(model) = provider.models.iter().find(|m| m.alias == alias) {
                return Some(model.model.clone());
            }
        }
        None
    }
}
