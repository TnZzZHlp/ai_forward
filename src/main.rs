use axum::{
    extract::DefaultBodyLimit,
    routing::{get, post},
    Router,
};
use clap::Parser;
use std::net::SocketAddr;
use tokio::signal;
use tower::ServiceBuilder;
use tower_http::trace::TraceLayer;
use tracing::info;

mod config;
mod error;
mod handlers;
mod logger;
mod middleware;
mod services;
mod state;

use config::Config;
use handlers::{chat, stats};
use middleware::auth_handler;
use state::AppState;

#[derive(Parser, Debug)]
struct Args {
    #[clap(short, long)]
    config: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // 设置配置文件路径
    if let Some(config_path) = args.config {
        std::env::set_var("CONFIG_PATH", config_path);
    }

    // 初始化配置
    let config = Config::new().map_err(error::AppError::Config)?;

    // 初始化日志
    logger::init_logging(&config).await;

    // 初始化应用状态
    let app_state = AppState::new(config.clone()).await?;
    info!("Application initialized successfully");

    // 创建路由
    let app = create_router(app_state);

    // 启动服务器
    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    info!("Server starting on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    let server = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal());

    server.await?;

    Ok(())
}

fn create_router(app_state: AppState) -> Router {
    let ai_routes = Router::new().nest(
        "/v1",
        Router::new()
            .route("/chat/completions", post(chat::chat_completions))
            .route("/embeddings", post(chat::embeddings))
            .route("/models", get(chat::list_models))
            .layer(axum::middleware::from_fn_with_state(
                app_state.clone(),
                auth_handler,
            )),
    );

    let manage_routes = Router::new()
        .route("/stats", get(stats::get_stats))
        .route("/reset", get(stats::reset_stats));

    Router::new()
        .merge(ai_routes)
        .merge(manage_routes)
        .layer(
            ServiceBuilder::new().layer(
                TraceLayer::new_for_http()
                    .make_span_with(
                        tower_http::trace::DefaultMakeSpan::new().level(tracing::Level::ERROR),
                    )
                    .on_request(
                        tower_http::trace::DefaultOnRequest::new().level(tracing::Level::DEBUG),
                    )
                    .on_response(
                        tower_http::trace::DefaultOnResponse::new().level(tracing::Level::INFO),
                    )
                    .on_failure(
                        tower_http::trace::DefaultOnFailure::new().level(tracing::Level::ERROR),
                    ),
            ),
        )
        .with_state(app_state)
        .layer(DefaultBodyLimit::max(100 * 1024 * 1024)) // 设置请求体最大为100MB
}

/// 监听停止信号的异步函数
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c().await.expect("安装 Ctrl+C 处理器失败");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("安装 SIGTERM 处理器失败")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("收到 Ctrl+C 信号，开始停止服务器...");
        },
        _ = terminate => {
            tracing::info!("收到 SIGTERM 信号，开始停止服务器...");
        },
    }
}

