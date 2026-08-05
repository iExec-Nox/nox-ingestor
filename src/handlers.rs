//! Axum server handlers for health checks and metrics.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use axum::{
    Json,
    extract::{Extension, State},
    http::{StatusCode, Uri},
    response::IntoResponse,
};
use axum_prometheus::metrics_exporter_prometheus::PrometheusHandle;
use chrono::Utc;
use serde_json::{Value, json};

/// Tracks ingestion liveness for `/health`. [`Application::run`](crate::application::Application::run)
/// calls [`record_success`](Self::record_success) each time the main loop makes forward
/// progress — a batch completed or the reader is simply caught up to the chain head with
/// nothing new to do. Both count as healthy; only a reader stuck retrying the same range (e.g.
/// the irreducible case in [`crate::chain::client`]) leaves this stale.
///
/// Stores the last-success timestamp as millis-since-`created_at` in an `AtomicU64` rather than
/// behind a `Mutex<Instant>`: `record_success` runs directly on the main ingestion loop (not a
/// spawned task), so a panic there would take down the whole process, not just this feature.
/// Atomics can't be poisoned, so that failure mode doesn't exist at all. `Relaxed` ordering is
/// sufficient: there's a single sequential writer, and no other shared state whose visibility
/// needs to be correlated with this value. `created_at` is only a shared coordinate origin so
/// two live `Instant` readings become comparable integers — both `record_success` and
/// `stale_for` re-derive their offset from it fresh on every call, so it cancels out in the
/// subtraction below; this does not measure time-since-creation.
pub struct IngestionLiveness {
    created_at: Instant,
    last_success_millis: AtomicU64,
    stall_threshold: Duration,
}

impl IngestionLiveness {
    pub fn new(stall_threshold: Duration) -> Self {
        Self {
            created_at: Instant::now(),
            last_success_millis: AtomicU64::new(0),
            stall_threshold,
        }
    }

    pub fn record_success(&self) {
        let millis = self.created_at.elapsed().as_millis() as u64;
        self.last_success_millis.store(millis, Ordering::Relaxed);
    }

    /// How long it's been since the last recorded success, if that exceeds `stall_threshold`.
    fn stale_for(&self) -> Option<Duration> {
        let now_millis = self.created_at.elapsed().as_millis() as u64;
        let last_millis = self.last_success_millis.load(Ordering::Relaxed);
        let elapsed = Duration::from_millis(now_millis.saturating_sub(last_millis));
        (elapsed > self.stall_threshold).then_some(elapsed)
    }
}

/// Health check endpoint handler.
///
/// Returns `200 {"status": "ok"}` while ingestion is making progress, or `503
/// {"status": "unhealthy", "seconds_since_last_batch": N}` once [`IngestionLiveness`] has gone
/// stale for longer than `app.health_stall_threshold`.
pub async fn health_check(
    Extension(liveness): Extension<Arc<IngestionLiveness>>,
) -> impl IntoResponse {
    match liveness.stale_for() {
        None => (StatusCode::OK, Json(json!({ "status": "ok" }))),
        Some(elapsed) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "status": "unhealthy",
                "seconds_since_last_batch": elapsed.as_secs(),
            })),
        ),
    }
}

/// `GET /metrics` — renders Prometheus metrics as plain text.
pub async fn metrics(State(metrics_handle): State<PrometheusHandle>) -> String {
    metrics_handle.render()
}

/// Fallback handler for non-existing routes.
///
/// Returns 404 NOT_FOUND to indicate the requested route does not exist.
pub async fn not_found(uri: Uri) -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error":format!("Route not found {}", uri.path()) })),
    )
}

/// `GET /` — returns service name and current UTC timestamp.
pub async fn root() -> Json<Value> {
    Json(json!({
        "service": "Ingestor",
        "timestamp": Utc::now().to_rfc3339()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn health_is_ok_when_liveness_is_fresh() {
        let liveness = Arc::new(IngestionLiveness::new(Duration::from_secs(60)));

        let response = health_check(Extension(liveness)).await.into_response();

        assert_eq!(StatusCode::OK, response.status());
    }

    #[tokio::test]
    async fn health_is_unavailable_once_stall_threshold_elapses() {
        let liveness = Arc::new(IngestionLiveness::new(Duration::from_millis(10)));
        tokio::time::sleep(Duration::from_millis(50)).await;

        let response = health_check(Extension(liveness)).await.into_response();

        assert_eq!(StatusCode::SERVICE_UNAVAILABLE, response.status());
    }

    #[tokio::test]
    async fn record_success_resets_staleness() {
        let liveness = Arc::new(IngestionLiveness::new(Duration::from_millis(10)));
        tokio::time::sleep(Duration::from_millis(50)).await;
        liveness.record_success();

        let response = health_check(Extension(liveness)).await.into_response();

        assert_eq!(StatusCode::OK, response.status());
    }
}
