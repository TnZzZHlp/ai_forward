use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::config::Config;
use crate::error::AppResult;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<Config>>,
    pub http_client: reqwest::Client,
    pub provider_usage: Arc<RwLock<DashMap<String, u64>>>,
    pub key_usage: Arc<RwLock<DashMap<String, u64>>>,
    pub ip_ban_manager: Arc<IpBanManager>,
}

/// IP封禁管理器
pub struct IpBanManager {
    /// 存储IP的失败次数
    fail_counts: DashMap<String, u32>,
    /// 存储被永久封禁的IP列表
    banned_ips: DashMap<String, ()>,
    /// 失败次数阈值
    max_failures: u32,
}

impl IpBanManager {
    pub fn new(max_failures: u32) -> Self {
        Self {
            fail_counts: DashMap::new(),
            banned_ips: DashMap::new(),
            max_failures,
        }
    }

    /// 检查IP是否被封禁
    pub fn is_banned(&self, ip: &str) -> bool {
        self.banned_ips.contains_key(ip)
    }

    /// 记录IP认证失败
    pub fn record_failure(&self, ip: &str) {
        let mut entry = self.fail_counts.entry(ip.to_string()).or_insert(0);
        *entry += 1;
        let count = *entry;
        drop(entry);

        // 如果失败次数达到阈值，永久封禁该IP
        if count >= self.max_failures {
            self.banned_ips.insert(ip.to_string(), ());
            tracing::warn!(
                "IP {} has been permanently banned after {} failed attempts",
                ip,
                count
            );
        } else {
            tracing::warn!(
                "IP {} failed authentication, attempts: {}/{}",
                ip,
                count,
                self.max_failures
            );
        }
    }

    /// 重置IP的失败次数（认证成功时调用）
    pub fn reset_failures(&self, ip: &str) {
        self.fail_counts.remove(ip);
    }

    /// 获取IP的失败次数
    pub fn get_failure_count(&self, ip: &str) -> u32 {
        self.fail_counts.get(ip).map(|v| *v).unwrap_or(0)
    }
}

impl AppState {
    pub async fn new(config: Config) -> AppResult<Self> {
        let http_client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()?;

        Ok(Self {
            config: Arc::new(RwLock::new(config)),
            http_client,
            provider_usage: Arc::new(RwLock::new(DashMap::new())),
            key_usage: Arc::new(RwLock::new(DashMap::new())),
            ip_ban_manager: Arc::new(IpBanManager::new(5)), // 失败5次封禁
        })
    }

    pub async fn reload_config(&self) -> AppResult<()> {
        let new_config = Config::new()?;
        let mut config_guard = self.config.write().await;
        *config_guard = new_config;
        Ok(())
    }

    pub async fn get_provider_by_model(&self, model: &str) -> Option<crate::config::Provider> {
        let config = self.config.read().await;
        config
            .providers
            .iter()
            .find(|provider| provider.models.iter().any(|m| m.alias == model))
            .cloned()
    }

    pub async fn get_model_mapping(&self, alias: &str) -> Option<String> {
        let config = self.config.read().await;
        for provider in &config.providers {
            if let Some(model) = provider.models.iter().find(|m| m.alias == alias) {
                return Some(model.model.clone());
            }
        }
        None
    }
}
