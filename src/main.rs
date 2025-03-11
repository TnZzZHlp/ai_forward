use dashmap::DashMap;
use once_cell::sync::OnceCell;
use quick_cache::sync::Cache;
use salvo::prelude::*;
use serde_json::Value;
use std::sync::{Arc, RwLock};

mod config;
use config::Config;

mod api;
use api::completions;

mod logger;
use logger::log;

mod db;
use db::{DatabaseClient, DB};

static CONFIG: OnceCell<Config> = OnceCell::new();
static CLIENT: OnceCell<reqwest::Client> = OnceCell::new();
static PROVIDER_USAGE_COUNT: OnceCell<DashMap<String, u64>> = OnceCell::new();
static KEY_USAGE_COUNT: OnceCell<DashMap<String, u64>> = OnceCell::new();
static CACHE: OnceCell<Cache<Value, Arc<String>>> = OnceCell::new();

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
    PROVIDER_USAGE_COUNT.set(DashMap::new()).unwrap();

    // Init Key Usage Count
    KEY_USAGE_COUNT.set(DashMap::new()).unwrap();

    // Init Logger
    tracing_subscriber::fmt::SubscriberBuilder::default()
        .with_timer(tracing_subscriber::fmt::time::ChronoLocal::rfc_3339())
        .with_max_level(tracing::Level::INFO)
        .pretty()
        .init();

    // Init DB
    DB.set(DatabaseClient::init(CONFIG.get().unwrap().database.clone()).await)
        .expect("Failed to init DB");

    // Init Cache
    CACHE
        .set(Cache::new(CONFIG.get().unwrap().cache_size as usize))
        .unwrap();
    // 加载缓存
    DB.get().unwrap().load_cache().await;
}
