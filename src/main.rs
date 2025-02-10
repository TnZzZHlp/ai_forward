use salvo::prelude::*;
use std::{collections::HashMap, sync::RwLock};

use once_cell::sync::OnceCell;

mod config;
use config::Config;

mod api;
use api::{completions, no_think_completions};

mod logger;
use logger::log;

static CONFIG: OnceCell<Config> = OnceCell::new();
static CLIENT: OnceCell<reqwest::Client> = OnceCell::new();
static PROVIDER_USAGE_COUNT: OnceCell<RwLock<HashMap<String, u64>>> = OnceCell::new();
static KEY_USAGE_COUNT: OnceCell<RwLock<HashMap<String, u64>>> = OnceCell::new();

#[tokio::main]
async fn main() {
    // Init Source
    init_source().await;

    // Start Server
    let router = Router::new().push(
        Router::with_path("v1").push(
            Router::with_path("chat")
                .push(Router::with_path("completions").post(completions))
                .push(Router::with_path("no_think_completions").post(no_think_completions)),
        ),
    );

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
    tracing_subscriber::fmt::init();
}
