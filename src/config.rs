use serde::Deserialize;
use std::env;
use std::fs;
use thiserror::Error;

#[derive(Debug, Error)]
#[error("{0}")]
pub struct ConfigError(pub String);

pub type ConfigResult<T> = Result<T, ConfigError>;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub auth: String,
    pub port: u16,
    pub providers: Vec<Provider>,
    pub log: Option<LogConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct LogConfig {
    pub level: String,
    pub file: String,
    pub max_files: Option<usize>,
    pub max_file_size: Option<u64>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Provider {
    pub name: String,
    pub models: Vec<Model>,
    pub url: String,
    pub keys: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Model {
    pub alias: String,
    pub model: String,
}

impl Config {
    pub fn new() -> ConfigResult<Self> {
        let config_path = env::var("CONFIG_PATH").unwrap_or_else(|_| "./config.json".to_string());

        let config_content = fs::read_to_string(&config_path).map_err(|e| {
            ConfigError(format!(
                "Failed to read config file '{}': {}",
                config_path, e
            ))
        })?;

        let config: Config = serde_json::from_str(&config_content)
            .map_err(|e| ConfigError(format!("Failed to parse config: {}", e)))?;

        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> ConfigResult<()> {
        if self.auth.is_empty() {
            return Err(ConfigError("Auth token cannot be empty".to_string()));
        }

        if self.providers.is_empty() {
            return Err(ConfigError(
                "At least one provider must be configured".to_string(),
            ));
        }

        for provider in &self.providers {
            if provider.keys.is_empty() {
                return Err(ConfigError(format!(
                    "Provider '{}' must have at least one API key",
                    provider.name
                )));
            }
            if provider.models.is_empty() {
                return Err(ConfigError(format!(
                    "Provider '{}' must have at least one model",
                    provider.name
                )));
            }
        }

        Ok(())
    }
}
