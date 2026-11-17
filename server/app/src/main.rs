use std::sync::Arc;

use gh_server::{
    AppState, DRAIN_TIMEOUT, app, auth::AllowAll, config::Config, db, events, hub::Hub, metrics,
    serve, shutdown_signal, ws,
};
use tokio::net::TcpListener;
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
    let metrics = metrics::install();

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

    let hub = Arc::new(Hub::default());
    let (events, _subscriber) = events::connect(&config.redis_url, hub.clone())
        .await
        .unwrap_or_else(|e| fail(&format!("can't connect to Redis: {e}")));

    let listener = bind(config.http_port).await;
    let metrics_listener = bind(config.metrics_port).await;
    tracing::info!(
        port = config.http_port,
        metrics_port = config.metrics_port,
        "listening"
    );
    let metrics_pool = pool.clone();
    tokio::spawn(async move {
        let router = metrics::router(metrics, metrics_pool);
        if let Err(e) = axum::serve(metrics_listener, router).await {
            tracing::error!("metrics server stopped: {e}");
        }
    });

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
        events,
        hub,
        authorizer: Arc::new(AllowAll),
        ws: ws::Settings::default(),
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

async fn bind(port: u16) -> TcpListener {
    TcpListener::bind(("0.0.0.0", port))
        .await
        .unwrap_or_else(|e| fail(&format!("can't listen on port {port}: {e}")))
}

fn fail(message: &str) -> ! {
    tracing::error!("{message}");
    std::process::exit(1);
}
