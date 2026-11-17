//! gh-core API server.

pub mod config;
pub mod db;

use std::time::Duration;

use axum::{
    Router,
    extract::State,
    http::{HeaderValue, Request, StatusCode, header::RETRY_AFTER},
    middleware,
    response::Response,
    routing::get,
};
use sqlx::PgPool;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tower_http::{
    compression::CompressionLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    timeout::TimeoutLayer,
    trace::{DefaultOnResponse, TraceLayer},
};
use tracing::{Level, info_span};

/// Upper bound on any HTTP request.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
/// After SIGTERM, the most time the process has before it exits.
pub const DRAIN_TIMEOUT: Duration = Duration::from_secs(15);
/// How long each dependency check in `/readyz` may take.
pub const READY_CHECK_TIMEOUT: Duration = Duration::from_millis(500);

/// Shared by every request handler. Cloning is cheap: each field is a handle.
#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    /// Cancelled when shutdown starts.
    pub shutdown: CancellationToken,
}

/// The routes, before middleware. Tests add their own routes to this.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/healthz", get(|| async { StatusCode::OK }))
        .route("/readyz", get(readyz))
}

/// Wraps a router in the middleware every request goes through.
pub fn with_middleware(router: Router) -> Router {
    // The last `.layer` call runs first on each request: it gets its ID, is
    // logged, gets Retry-After if it ends up a 503, is timed out, and is
    // compressed on the way back.
    router
        .layer(CompressionLayer::new())
        .layer(TimeoutLayer::with_status_code(
            StatusCode::SERVICE_UNAVAILABLE,
            REQUEST_TIMEOUT,
        ))
        .layer(middleware::map_response(add_retry_after))
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

pub fn app(state: AppState) -> Router {
    with_middleware(routes().with_state(state))
}

/// Every 503 tells the client when to retry, whichever layer produced it.
async fn add_retry_after(mut res: Response) -> Response {
    if res.status() == StatusCode::SERVICE_UNAVAILABLE {
        res.headers_mut()
            .entry(RETRY_AFTER)
            .or_insert(HeaderValue::from_static("1"));
    }
    res
}

/// 200 when this instance should get traffic: not shutting down, and Postgres
/// answers within the check timeout.
async fn readyz(State(state): State<AppState>) -> StatusCode {
    if state.shutdown.is_cancelled() {
        return StatusCode::SERVICE_UNAVAILABLE;
    }
    let ping = sqlx::query("SELECT 1").execute(&state.db);
    match tokio::time::timeout(READY_CHECK_TIMEOUT, ping).await {
        Ok(Ok(_)) => StatusCode::OK,
        _ => StatusCode::SERVICE_UNAVAILABLE,
    }
}

/// Serves `app` until `shutdown` is cancelled, then:
/// 1. `/readyz` already reports 503 (it checks the same token);
/// 2. after `delay`, stops accepting new connections;
/// 3. lets in-flight requests finish, but returns no later than `drain_limit`
///    after `shutdown` was cancelled.
pub async fn serve(
    listener: TcpListener,
    app: Router,
    shutdown: CancellationToken,
    delay: Duration,
    drain_limit: Duration,
) -> std::io::Result<()> {
    let stop_accepting = {
        let shutdown = shutdown.clone();
        async move {
            shutdown.cancelled().await;
            tokio::time::sleep(delay).await;
        }
    };
    let server = axum::serve(listener, app).with_graceful_shutdown(stop_accepting);

    tokio::select! {
        result = server => result,
        () = async {
            shutdown.cancelled().await;
            tokio::time::sleep(drain_limit).await;
        } => {
            tracing::warn!("drain limit reached, exiting with requests still open");
            Ok(())
        }
    }
}

/// Resolves on SIGTERM (what Docker and other orchestrators send) or Ctrl+C.
pub async fn shutdown_signal() {
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
}
