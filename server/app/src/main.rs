//! gh-core API server.

mod config;

use std::time::Duration;

use axum::{
    Router,
    http::{Request, StatusCode},
    routing::get,
};
use tokio_util::sync::CancellationToken;
use tower_http::{
    compression::CompressionLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    timeout::TimeoutLayer,
    trace::{DefaultOnResponse, TraceLayer},
};
use tracing::{Level, info_span};
use tracing_subscriber::EnvFilter;

// musl's default allocator is slow when many threads allocate at once, so the
// whole program uses mimalloc instead.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// Upper bound on any HTTP request.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
/// After SIGTERM, how long in-flight requests get to finish before we exit.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(15);

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .json()
        .with_current_span(true)
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let config = config::Config::from_env().unwrap_or_else(|errors| {
        for error in errors {
            tracing::error!("{error}");
        }
        std::process::exit(1);
    });

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", config.http_port))
        .await
        .unwrap_or_else(|e| {
            tracing::error!("can't listen on port {}: {e}", config.http_port);
            std::process::exit(1);
        });
    tracing::info!(port = config.http_port, "listening");

    // Cancelled once SIGTERM (or Ctrl+C) arrives. Later steps hand clones of
    // this token to the WebSocket hub so it can close connections too.
    let stop = CancellationToken::new();
    tokio::spawn({
        let stop = stop.clone();
        async move {
            shutdown_signal().await;
            stop.cancel();
        }
    });

    let server =
        axum::serve(listener, app()).with_graceful_shutdown(stop.clone().cancelled_owned());
    tokio::select! {
        result = server => {
            if let Err(e) = result {
                tracing::error!("server error: {e}");
                std::process::exit(1);
            }
        }
        () = async {
            stop.cancelled().await;
            tokio::time::sleep(DRAIN_TIMEOUT).await;
        } => tracing::warn!("drain timeout reached, exiting with requests still open"),
    }
    tracing::info!("stopped");
}

fn app() -> Router {
    // Layers wrap the routes from the inside out: the last `.layer` call runs
    // first on each request. So a request gets its ID, then is logged, then
    // is timed out, then is compressed on the way back.
    Router::new()
        .route("/healthz", get(|| async { StatusCode::OK }))
        .layer(CompressionLayer::new())
        .layer(TimeoutLayer::with_status_code(
            StatusCode::SERVICE_UNAVAILABLE,
            REQUEST_TIMEOUT,
        ))
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|req: &Request<_>| {
                    let request_id = req
                        .headers()
                        .get("x-request-id")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("-");
                    info_span!("request", method = %req.method(), path = req.uri().path(), request_id)
                })
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
}

/// Resolves on SIGTERM (what Docker and other orchestrators send) or Ctrl+C.
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut term = signal(SignalKind::terminate()).expect("install SIGTERM handler");
        tokio::select! {
            _ = term.recv() => {}
            _ = tokio::signal::ctrl_c() => {}
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c()
        .await
        .expect("install Ctrl+C handler");

    tracing::info!("shutdown signal received, draining");
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use tower::ServiceExt;

    #[tokio::test]
    async fn healthz_returns_ok_with_request_id() {
        let res = app()
            .oneshot(Request::get("/healthz").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert!(res.headers().contains_key("x-request-id"));
    }
}
