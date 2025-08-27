use axum::{
    response::Json,
    routing::{get, post},
    Router,
};
use clap::Parser;
use std::net::SocketAddr;
use tower::ServiceBuilder;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::info;
use tracing_subscriber::{prelude::*, Layer};

mod config;
mod error;
mod handlers;
mod middleware;
mod models;
mod services;
mod state;

use config::Config;
use error::AppResult;
use handlers::{chat, stats};
use middleware::{auth_handler, logging_handler, response_time_handler};
use state::AppState;

#[derive(Parser, Debug)]
struct Args {
    #[clap(short, long)]
    config: Option<String>,
}

#[tokio::main]
async fn main() -> AppResult<()> {
    let args = Args::parse();

    // 设置配置文件路径
    if let Some(config_path) = args.config {
        std::env::set_var("CONFIG_PATH", config_path);
    }

    // 初始化配置
    let config = Config::new().map_err(error::AppError::Config)?;

    // 初始化日志
    init_logging(&config).await;

    // 初始化应用状态
    let app_state = AppState::new(config.clone()).await?;
    info!("Application initialized successfully");

    // 创建路由
    let app = create_router(app_state);

    // 启动服务器
    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    info!("Server starting on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();

    Ok(())
}

fn create_router(app_state: AppState) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .nest(
            "/v1",
            Router::new()
                .route("/chat/completions", post(chat::chat_completions))
                .route("/models", get(chat::list_models))
                .layer(axum::middleware::from_fn_with_state(
                    app_state.clone(),
                    auth_handler,
                )),
        )
        .nest(
            "/admin",
            Router::new()
                .route("/stats", get(stats::get_stats))
                .route("/reset", post(stats::reset_stats))
                .layer(axum::middleware::from_fn_with_state(
                    app_state.clone(),
                    auth_handler,
                )),
        )
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .layer(CorsLayer::permissive())
                .layer(axum::middleware::from_fn(logging_handler))
                .layer(axum::middleware::from_fn(response_time_handler)),
        )
        .with_state(app_state)
}

async fn health_check() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "timestamp": chrono::Utc::now().to_rfc3339()
    }))
}

async fn init_logging(config: &Config) {
    let level = config
        .log
        .as_ref()
        .map_or("info".to_string(), |l| l.level.clone())
        .parse::<tracing_subscriber::filter::LevelFilter>()
        .unwrap_or(tracing_subscriber::filter::LevelFilter::INFO);

    let mut layers = Vec::new();

    // 控制台日志
    layers.push(
        tracing_subscriber::fmt::layer()
            .with_target(false)
            .with_timer(tracing_subscriber::fmt::time::ChronoLocal::new(
                String::from("%Y-%m-%d %H:%M:%S"),
            ))
            .with_writer(std::io::stdout)
            .with_filter(level)
            .boxed(),
    );

    // 文件日志
    if let Some(log) = &config.log {
        let log = log.clone();

        use file_rotate::{compression::*, suffix::*, *};

        let file_layer = tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_target(false)
            .with_timer(tracing_subscriber::fmt::time::ChronoLocal::new(
                String::from("%Y-%m-%d %H:%M:%S"),
            ))
            .with_writer(move || {
                let log_file = log.file.clone();
                FileRotate::new(
                    log_file,
                    AppendTimestamp::default(FileLimit::MaxFiles(log.max_files.unwrap_or(3))),
                    ContentLimit::BytesSurpassed(
                        log.max_file_size.unwrap_or(10 * 1024 * 1024) as usize
                    ),
                    Compression::OnRotate(1),
                    None,
                )
            })
            .with_filter(level)
            .boxed();
        layers.push(file_layer);
    }

    tracing_subscriber::registry().with(layers).init();
}
