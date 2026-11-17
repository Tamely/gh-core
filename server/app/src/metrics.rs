//! Prometheus metrics, served on a separate internal port so they aren't public.

use std::time::Instant;

use axum::{
    Router,
    extract::{MatchedPath, Request, State},
    middleware::Next,
    response::Response,
    routing::get,
};
use metrics::{counter, gauge, histogram};
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};
use sqlx::PgPool;

/// Bucket bounds in seconds, from 100µs to 10s.
const BUCKETS: &[f64] = &[
    0.0001, 0.00025, 0.0005, 0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5,
    5.0, 10.0,
];

/// Installs the global recorder. Call once, inside the Tokio runtime, before
/// anything records.
pub fn install() -> PrometheusHandle {
    let handle = PrometheusBuilder::new()
        .set_buckets_for_metric(Matcher::Suffix("_seconds".into()), BUCKETS)
        .expect("buckets are valid")
        .install_recorder()
        .expect("metrics recorder installs once");

    // Histograms keep every raw sample until upkeep folds them into buckets.
    // Without this, memory grows with every event until someone scrapes.
    let upkeep = handle.clone();
    tokio::spawn(async move {
        let mut every = tokio::time::interval(std::time::Duration::from_secs(5));
        loop {
            every.tick().await;
            upkeep.run_upkeep();
        }
    });

    // Register every series at zero so dashboards and alerts see them before
    // the first event happens.
    for name in [
        "ws_messages_sent_total",
        "ws_resyncs_total",
        "ws_slow_client_disconnects_total",
        "redis_reconnects_total",
    ] {
        counter!(name).absolute(0);
    }
    for name in ["ws_connections", "db_pool_waiting", "redis_connected"] {
        gauge!(name).set(0.0);
    }
    for kind in crate::topic::TopicKind::ALL {
        gauge!("ws_subscriptions", "kind" => kind.as_str()).set(0.0);
    }
    handle
}

/// The internal `/metrics` endpoint.
pub fn router(handle: PrometheusHandle, pool: PgPool) -> Router {
    Router::new()
        .route("/metrics", get(render))
        .with_state((handle, pool))
}

async fn render(State((handle, pool)): State<(PrometheusHandle, PgPool)>) -> String {
    gauge!("db_pool_connections").set(pool.size() as f64);
    gauge!("db_pool_idle_connections").set(pool.num_idle() as f64);
    handle.render()
}

/// Records each request's latency, labelled by route pattern (`/rooms/{id}`,
/// not `/rooms/42`, so the number of series stays small) and status.
pub async fn track_http(req: Request, next: Next) -> Response {
    let route = req
        .extensions()
        .get::<MatchedPath>()
        .map_or_else(|| "unmatched".to_owned(), |p| p.as_str().to_owned());
    let started = Instant::now();
    let res = next.run(req).await;
    histogram!(
        "http_request_duration_seconds",
        "route" => route,
        "status" => res.status().as_str().to_owned(),
    )
    .record(started.elapsed());
    res
}
