use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub auth: String,
    pub port: u16,
    pub providers: Vec<Provider>,
}

#[derive(Debug, Deserialize)]
pub struct Provider {
    pub name: String,
    pub models: Vec<Model>,
    pub url: String,
    pub keys: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct Model {
    pub alias: String,
    pub model: String,
    pub think: Option<bool>,
}

impl Config {
    pub fn new() -> Self {
        // 读取配置文件
        let config = std::fs::read_to_string("./config.json").unwrap();
        serde_json::from_str(&config).unwrap()
    }
}
