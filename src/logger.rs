use tracing_subscriber::{prelude::*, Layer};

use crate::config::Config;

pub async fn init_logging(config: &Config) {
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
            .compact()
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
            .compact()
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
