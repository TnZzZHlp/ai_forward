use moka::future::Cache;
use once_cell::sync::OnceCell;
use salvo::prelude::*;
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

mod config;
use config::Config;

mod api;
use api::completions;

mod logger;
use logger::log;

static CONFIG: OnceCell<Config> = OnceCell::new();
static CLIENT: OnceCell<reqwest::Client> = OnceCell::new();
static PROVIDER_USAGE_COUNT: OnceCell<RwLock<HashMap<String, u64>>> = OnceCell::new();
static KEY_USAGE_COUNT: OnceCell<RwLock<HashMap<String, u64>>> = OnceCell::new();
static CACHE: OnceCell<Cache<String, Arc<String>>> = OnceCell::new();

#[tokio::main]
async fn main() {
    // Init Source
    init_source().await;

    // Start Server
    let router =
        Router::new().push(Router::with_path("v1").push(
            Router::with_path("chat").push(Router::with_path("completions").post(completions)),
        ));

    let service = Service::new(router).hoop(log);

    let acceptor = TcpListener::new(format!("0.0.0.0:{}", CONFIG.get().unwrap().port))
        .bind()
        .await;

    Server::new(acceptor).serve(service).await;
}

async fn init_source() {
    // Init Config
    CONFIG.set(Config::new()).unwrap();

    // Init Client
    CLIENT
        .set(
            reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap(),
        )
        .unwrap();

    // Init Provider Usage Count
    PROVIDER_USAGE_COUNT
        .set(RwLock::new(HashMap::new()))
        .unwrap();

    // Init Key Usage Count
    KEY_USAGE_COUNT.set(RwLock::new(HashMap::new())).unwrap();

    // Init Logger
    tracing_subscriber::fmt::SubscriberBuilder::default()
        .with_timer(tracing_subscriber::fmt::time::ChronoLocal::rfc_3339())
        .with_max_level(tracing::Level::INFO)
        .init();

    // Init Cache
    CACHE.set(Cache::new(100000)).unwrap();
    // 如果有缓存文件就加载
    if std::path::Path::new("cache").exists() {
        let caches = std::fs::read_to_string("cache").unwrap();
        for cache in caches.split("\n+++\n") {
            if cache.is_empty() {
                continue;
            }

            let cache = cache.split("\n===\n").collect::<Vec<&str>>();

            let key = cache[0];
            let value = cache[1].to_string();

            CACHE
                .get()
                .unwrap()
                .insert(key.to_string(), Arc::new(value))
                .await;
        }
    }
}
