use gh_server::{AppState, DRAIN_TIMEOUT, app, config::Config, db, serve, shutdown_signal};
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

// musl's default allocator is slow when many threads allocate at once, so the
// whole program uses mimalloc instead.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .json()
        .with_current_span(true)
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let config = Config::from_env().unwrap_or_else(|errors| {
        for error in errors {
            tracing::error!("{error}");
        }
        fail("invalid configuration")
    });

    if let Err(e) = db::migrate(&config.database_url).await {
        fail(&format!("migrations failed: {e}"));
    }
    let pool = db::connect(&config)
        .await
        .unwrap_or_else(|e| fail(&format!("can't connect to Postgres: {e}")));

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", config.http_port))
        .await
        .unwrap_or_else(|e| fail(&format!("can't listen on port {}: {e}", config.http_port)));
    tracing::info!(port = config.http_port, "listening");

    let shutdown = CancellationToken::new();
    tokio::spawn({
        let shutdown = shutdown.clone();
        async move {
            shutdown_signal().await;
            tracing::info!("shutdown signal received, draining");
            shutdown.cancel();
        }
    });

    let state = AppState {
        db: pool.clone(),
        shutdown: shutdown.clone(),
    };
    if let Err(e) = serve(
        listener,
        app(state),
        shutdown,
        config.shutdown_delay,
        DRAIN_TIMEOUT,
    )
    .await
    {
        fail(&format!("server error: {e}"));
    }
    pool.close().await;
    tracing::info!("stopped");
}

fn fail(message: &str) -> ! {
    tracing::error!("{message}");
    std::process::exit(1);
}
