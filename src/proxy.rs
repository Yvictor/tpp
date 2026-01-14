use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use pingora::prelude::*;
use pingora_proxy::{ProxyHttp, Session};
use tracing::{debug, info};

use crate::token_pool::{Token, TokenPool};

/// HTTP proxy that injects Bearer tokens from a pool
pub struct TokenPoolProxy {
    /// The token pool
    pool: Arc<TokenPool>,
    /// Upstream server address (host:port)
    upstream: String,
    /// Whether to use TLS for upstream connection
    use_tls: bool,
}

/// Per-connection context
pub struct ProxyCtx {
    /// The token acquired for this connection
    token: Option<Token>,
    /// When this connection started
    conn_start: Instant,
    /// Number of requests on this connection
    request_count: u64,
}

impl TokenPoolProxy {
    pub fn new(pool: Arc<TokenPool>, upstream: String, use_tls: bool) -> Self {
        Self {
            pool,
            upstream,
            use_tls,
        }
    }
}

#[async_trait]
impl ProxyHttp for TokenPoolProxy {
    type CTX = ProxyCtx;

    fn new_ctx(&self) -> Self::CTX {
        ProxyCtx {
            token: None,
            conn_start: Instant::now(),
            request_count: 0,
        }
    }

    /// Select upstream peer and acquire token on first request
    async fn upstream_peer(
        &self,
        _session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>> {
        // Acquire token on first request of this connection
        if ctx.token.is_none() {
            let token = self.pool.acquire().await;
            info!(
                "Connection acquired token #{} (pool: {}/{} in use)",
                token.id,
                self.pool.in_use(),
                self.pool.total()
            );
            ctx.token = Some(token);
        }

        ctx.request_count += 1;

        let peer = HttpPeer::new(&self.upstream, self.use_tls, self.upstream.clone());

        Ok(Box::new(peer))
    }

    /// Inject Authorization header before sending to upstream
    async fn upstream_request_filter(
        &self,
        _session: &mut Session,
        upstream_request: &mut pingora::http::RequestHeader,
        ctx: &mut Self::CTX,
    ) -> Result<()> {
        if let Some(ref token) = ctx.token {
            upstream_request
                .insert_header("Authorization", format!("Bearer {}", token.value))
                .map_err(|e| {
                    pingora::Error::because(
                        pingora::ErrorType::InternalError,
                        "Failed to insert Authorization header",
                        e,
                    )
                })?;

            debug!("Injected Authorization header for token #{}", token.id);
        }

        Ok(())
    }

    /// Called when request completes (success or error)
    async fn logging(
        &self,
        session: &mut Session,
        e: Option<&pingora::Error>,
        ctx: &mut Self::CTX,
    ) {
        let duration = ctx.conn_start.elapsed();

        // Check if this was an error response
        let is_error = e.is_some()
            || session
                .response_written()
                .map_or(false, |resp| resp.status.as_u16() >= 400);

        if let Some(ref token) = ctx.token {
            if is_error {
                self.pool.mark_error(token);
            }
        }

        // Only release token when connection is closing
        // For HTTP/1.1 keep-alive, this happens when the connection ends
        if session.is_body_done() {
            if let Some(token) = ctx.token.take() {
                let token_id = token.id;
                self.pool.release(token);

                info!(
                    "Connection released token #{} after {} requests, duration: {:.2}s (pool: {}/{} in use)",
                    token_id,
                    ctx.request_count,
                    duration.as_secs_f64(),
                    self.pool.in_use(),
                    self.pool.total()
                );
            }
        }
    }
}
