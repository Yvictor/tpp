use std::sync::Arc;

use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use serde::Serialize;
use tokio::net::TcpListener;
use tracing::info;

use crate::token_pool::TokenPool;

/// Health check response
#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub pool: PoolStatus,
}

/// Token pool status
#[derive(Serialize)]
pub struct PoolStatus {
    pub total: usize,
    pub in_use: u64,
    pub available: usize,
    pub waiting: u64,
}

/// Application state for health check server
#[derive(Clone)]
pub struct HealthState {
    pool: Arc<TokenPool>,
}

impl HealthState {
    pub fn new(pool: Arc<TokenPool>) -> Self {
        Self { pool }
    }
}

/// Health check handler - returns 200 if healthy
async fn health_handler(State(state): State<HealthState>) -> impl IntoResponse {
    let pool_status = PoolStatus {
        total: state.pool.total(),
        in_use: state.pool.in_use(),
        available: state.pool.available(),
        waiting: state.pool.waiting(),
    };

    // Consider unhealthy if all tokens are in use and there are waiters
    let status = if state.pool.waiting() > 0 && state.pool.available() == 0 {
        "degraded"
    } else {
        "healthy"
    };

    let response = HealthResponse {
        status,
        pool: pool_status,
    };

    (StatusCode::OK, Json(response))
}

/// Liveness probe - always returns 200
async fn liveness_handler() -> impl IntoResponse {
    StatusCode::OK
}

/// Readiness probe - returns 200 if pool has available tokens
async fn readiness_handler(State(state): State<HealthState>) -> impl IntoResponse {
    if state.pool.total() > 0 {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}

/// Metrics handler - returns pool metrics in Prometheus format
async fn metrics_handler(State(state): State<HealthState>) -> impl IntoResponse {
    let metrics = format!(
        "# HELP tpp_tokens_total Total number of tokens in the pool\n\
         # TYPE tpp_tokens_total gauge\n\
         tpp_tokens_total {}\n\
         # HELP tpp_tokens_in_use Number of tokens currently in use\n\
         # TYPE tpp_tokens_in_use gauge\n\
         tpp_tokens_in_use {}\n\
         # HELP tpp_tokens_available Number of tokens available\n\
         # TYPE tpp_tokens_available gauge\n\
         tpp_tokens_available {}\n\
         # HELP tpp_requests_waiting Number of requests waiting for a token\n\
         # TYPE tpp_requests_waiting gauge\n\
         tpp_requests_waiting {}\n",
        state.pool.total(),
        state.pool.in_use(),
        state.pool.available(),
        state.pool.waiting(),
    );

    (
        StatusCode::OK,
        [("content-type", "text/plain; charset=utf-8")],
        metrics,
    )
}

/// Create the health check router
pub fn health_router(pool: Arc<TokenPool>) -> Router {
    let state = HealthState::new(pool);

    Router::new()
        .route("/health", get(health_handler))
        .route("/healthz", get(health_handler))
        .route("/livez", get(liveness_handler))
        .route("/readyz", get(readiness_handler))
        .route("/metrics", get(metrics_handler))
        .with_state(state)
}

/// Start the health check server
pub async fn start_health_server(
    addr: &str,
    pool: Arc<TokenPool>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let app = health_router(pool);
    let listener = TcpListener::bind(addr).await?;

    info!("Health check server listening on {}", addr);

    axum::serve(listener, app).await?;

    Ok(())
}

/// Spawn health check server as a background task
pub fn spawn_health_server(addr: String, pool: Arc<TokenPool>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        if let Err(e) = start_health_server(&addr, pool).await {
            tracing::error!("Health check server error: {}", e);
        }
    })
}
