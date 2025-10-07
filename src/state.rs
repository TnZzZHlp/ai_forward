use dashmap::DashMap;
use ipnet::IpNet;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
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
    /// 存储IPv4地址的失败记录（IP -> (失败次数, 第一次失败时间)）
    ipv4_fail_records: DashMap<String, (u32, Instant)>,
    /// 存储IPv6 /48网段的失败记录（网段 -> (失败次数, 第一次失败时间)）
    ipv6_fail_records: DashMap<String, (u32, Instant)>,
    /// 存储被永久封禁的IPv4地址列表
    banned_ipv4: DashMap<String, ()>,
    /// 存储被永久封禁的IPv6 /48网段列表
    banned_ipv6_networks: DashMap<String, ()>,
    /// 失败次数阈值
    max_failures: u32,
    /// 失败次数统计时间窗口（小时）
    failure_window_hours: u64,
}

impl IpBanManager {
    pub fn new(max_failures: u32) -> Self {
        Self {
            ipv4_fail_records: DashMap::new(),
            ipv6_fail_records: DashMap::new(),
            banned_ipv4: DashMap::new(),
            banned_ipv6_networks: DashMap::new(),
            max_failures,
            failure_window_hours: 1, // 1小时时间窗口
        }
    }

    /// 检查IP是否被封禁
    pub fn is_banned(&self, ip: &str) -> bool {
        // 尝试解析IP地址
        if let Ok(ip_addr) = ip.parse::<IpAddr>() {
            match ip_addr {
                IpAddr::V4(_) => {
                    // IPv4地址直接检查
                    self.banned_ipv4.contains_key(ip)
                }
                IpAddr::V6(_) => {
                    // IPv6地址检查是否在任何被封禁的/48网段中
                    self.is_ipv6_banned(&ip_addr)
                }
            }
        } else {
            // 如果IP地址解析失败，不封禁（避免误封）
            false
        }
    }

    /// 检查IPv6地址是否在任何被封禁的/48网段中
    fn is_ipv6_banned(&self, ip: &IpAddr) -> bool {
        if matches!(ip, IpAddr::V6(_)) {
            // 计算IPv6地址的/48网段
            if let Ok(network) = IpNet::new(*ip, 48) {
                let network_str = network.to_string();
                return self.banned_ipv6_networks.contains_key(&network_str);
            }
        }
        false
    }

    /// 获取IPv6地址的/48网段
    fn get_ipv6_network(ip: &str) -> Option<String> {
        if let Ok(ip_addr) = ip.parse::<IpAddr>() {
            if ip_addr.is_ipv6() {
                if let Ok(network) = IpNet::new(ip_addr, 48) {
                    return Some(network.to_string());
                }
            }
        }
        None
    }

    /// 记录IP认证失败
    pub fn record_failure(&self, ip: &str) {
        let now = Instant::now();
        let window_duration = Duration::from_secs(self.failure_window_hours * 3600);

        // 尝试解析IP地址
        if let Ok(ip_addr) = ip.parse::<IpAddr>() {
            match ip_addr {
                IpAddr::V4(_) => {
                    // IPv4地址处理
                    let mut entry = self
                        .ipv4_fail_records
                        .entry(ip.to_string())
                        .or_insert((0, now));
                    let (_count, first_failure_time) = *entry;

                    if now.duration_since(first_failure_time) <= window_duration {
                        entry.0 += 1;
                        let new_count = entry.0;

                        tracing::warn!(
                            "IPv4 {} failed authentication, attempts: {}/{} (within {}h window)",
                            ip,
                            new_count,
                            self.max_failures,
                            self.failure_window_hours
                        );

                        if new_count >= self.max_failures {
                            self.banned_ipv4.insert(ip.to_string(), ());
                            tracing::warn!(
                                "IPv4 {} has been permanently banned after {} failed attempts",
                                ip,
                                new_count
                            );
                        }
                    } else {
                        entry.0 = 1;
                        entry.1 = now;
                        tracing::warn!(
                            "IPv4 {} failed authentication, attempts: 1/{} (reset after time window)",
                            ip,
                            self.max_failures
                        );
                    }
                }
                IpAddr::V6(_) => {
                    // IPv6地址处理 - 使用/48网段作为键
                    if let Some(network) = Self::get_ipv6_network(ip) {
                        tracing::warn!("IPv6 {} belongs to network {}", ip, network);
                        let mut entry = self
                            .ipv6_fail_records
                            .entry(network.clone())
                            .or_insert((0, now));
                        let (_count, first_failure_time) = *entry;

                        if now.duration_since(first_failure_time) <= window_duration {
                            entry.0 += 1;
                            let new_count = entry.0;

                            tracing::warn!(
                                "IPv6 {} (network {}) failed authentication, attempts: {}/{} (within {}h window)",
                                ip,
                                network,
                                new_count,
                                self.max_failures,
                                self.failure_window_hours
                            );

                            if new_count >= self.max_failures {
                                self.banned_ipv6_networks.insert(network.clone(), ());
                                tracing::warn!(
                                    "IPv6 network {} (from IP {}) has been permanently banned after {} failed attempts",
                                    network,
                                    ip,
                                    new_count
                                );
                            }
                        } else {
                            entry.0 = 1;
                            entry.1 = now;
                            tracing::warn!(
                                "IPv6 {} (network {}) failed authentication, attempts: 1/{} (reset after time window)",
                                ip,
                                network,
                                self.max_failures
                            );
                        }
                    } else {
                        tracing::warn!("Failed to calculate network for IPv6 {}", ip);
                        // 如果无法计算网段，按单个IP处理
                        let mut entry = self
                            .ipv4_fail_records
                            .entry(ip.to_string())
                            .or_insert((0, now));
                        let (_count, first_failure_time) = *entry;

                        if now.duration_since(first_failure_time) <= window_duration {
                            entry.0 += 1;
                            let new_count = entry.0;

                            tracing::warn!(
                                "IPv6 {} failed authentication, attempts: {}/{} (within {}h window)",
                                ip,
                                new_count,
                                self.max_failures,
                                self.failure_window_hours
                            );

                            if new_count >= self.max_failures {
                                self.banned_ipv4.insert(ip.to_string(), ());
                                tracing::warn!(
                                    "IPv6 {} has been permanently banned after {} failed attempts",
                                    ip,
                                    new_count
                                );
                            }
                        } else {
                            entry.0 = 1;
                            entry.1 = now;
                            tracing::warn!(
                                "IPv6 {} failed authentication, attempts: 1/{} (reset after time window)",
                                ip,
                                self.max_failures
                            );
                        }
                    }
                }
            }
        } else {
            // 如果IP地址解析失败，按原始字符串处理
            let mut entry = self
                .ipv4_fail_records
                .entry(ip.to_string())
                .or_insert((0, now));
            let (_count, first_failure_time) = *entry;

            if now.duration_since(first_failure_time) <= window_duration {
                entry.0 += 1;
                let new_count = entry.0;

                tracing::warn!(
                    "IP {} failed authentication, attempts: {}/{} (within {}h window)",
                    ip,
                    new_count,
                    self.max_failures,
                    self.failure_window_hours
                );

                if new_count >= self.max_failures {
                    self.banned_ipv4.insert(ip.to_string(), ());
                    tracing::warn!(
                        "IP {} has been permanently banned after {} failed attempts",
                        ip,
                        new_count
                    );
                }
            } else {
                entry.0 = 1;
                entry.1 = now;
                tracing::warn!(
                    "IP {} failed authentication, attempts: 1/{} (reset after time window)",
                    ip,
                    self.max_failures
                );
            }
        }
    }

    /// 重置IP的失败记录（认证成功时调用）
    pub fn reset_failures(&self, ip: &str) {
        if let Ok(ip_addr) = ip.parse::<IpAddr>() {
            match ip_addr {
                IpAddr::V4(_) => {
                    self.ipv4_fail_records.remove(ip);
                }
                IpAddr::V6(_) => {
                    if let Some(network) = Self::get_ipv6_network(ip) {
                        self.ipv6_fail_records.remove(&network);
                    } else {
                        self.ipv4_fail_records.remove(ip);
                    }
                }
            }
        } else {
            self.ipv4_fail_records.remove(ip);
        }
        tracing::info!("IP {} authentication successful, failure record reset", ip);
    }

    /// 获取IP的失败次数
    pub fn get_failure_count(&self, ip: &str) -> u32 {
        if let Ok(ip_addr) = ip.parse::<IpAddr>() {
            match ip_addr {
                IpAddr::V4(_) => {
                    if let Some(record) = self.ipv4_fail_records.get(ip) {
                        let (count, first_failure_time) = *record;
                        let now = Instant::now();
                        let window_duration = Duration::from_secs(self.failure_window_hours * 3600);

                        if now.duration_since(first_failure_time) <= window_duration {
                            count
                        } else {
                            0
                        }
                    } else {
                        0
                    }
                }
                IpAddr::V6(_) => {
                    if let Some(network) = Self::get_ipv6_network(ip) {
                        if let Some(record) = self.ipv6_fail_records.get(&network) {
                            let (count, first_failure_time) = *record;
                            let now = Instant::now();
                            let window_duration =
                                Duration::from_secs(self.failure_window_hours * 3600);

                            if now.duration_since(first_failure_time) <= window_duration {
                                count
                            } else {
                                0
                            }
                        } else {
                            0
                        }
                    } else if let Some(record) = self.ipv4_fail_records.get(ip) {
                        let (count, first_failure_time) = *record;
                        let now = Instant::now();
                        let window_duration = Duration::from_secs(self.failure_window_hours * 3600);

                        if now.duration_since(first_failure_time) <= window_duration {
                            count
                        } else {
                            0
                        }
                    } else {
                        0
                    }
                }
            }
        } else if let Some(record) = self.ipv4_fail_records.get(ip) {
            let (count, first_failure_time) = *record;
            let now = Instant::now();
            let window_duration = Duration::from_secs(self.failure_window_hours * 3600);

            if now.duration_since(first_failure_time) <= window_duration {
                count
            } else {
                0
            }
        } else {
            0
        }
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

        // 检查是否是 provider:model 格式
        if let Some((provider_name, _model_name)) = model.split_once(':') {
            // 如果是 provider:model 格式，直接查找对应的provider
            config
                .providers
                .iter()
                .find(|provider| provider.name == provider_name)
                .cloned()
        } else {
            // 如果不是 provider:model 格式，使用原来的查找逻辑
            config
                .providers
                .iter()
                .find(|provider| provider.models.iter().any(|m| m.alias == model))
                .cloned()
        }
    }

    pub async fn get_model_mapping(&self, alias: &str) -> Option<String> {
        let config = self.config.read().await;

        // 检查是否是 provider:model 格式
        if let Some((provider_name, model_name)) = alias.split_once(':') {
            // 如果是 provider:model 格式，查找对应的provider和model
            for provider in &config.providers {
                if provider.name == provider_name {
                    // 如果provider找到了，返回model名称（即冒号后面的部分）
                    return Some(model_name.to_string());
                }
            }
            None
        } else {
            // 如果不是 provider:model 格式，使用原来的查找逻辑
            for provider in &config.providers {
                if let Some(model) = provider.models.iter().find(|m| m.alias == alias) {
                    return Some(model.model.clone());
                }
            }
            None
        }
    }
}
