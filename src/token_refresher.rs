use std::sync::Arc;
use std::time::Duration;

use tokio::time::{interval, timeout};
use tracing::{debug, error, info, warn};

use crate::token_acquirer::TokenAcquirer;
use crate::token_pool::TokenPool;

/// Background task that refreshes tokens
pub struct TokenRefresher {
    pool: Arc<TokenPool>,
    acquirer: TokenAcquirer,
    /// Token TTL - refresh tokens older than this
    ttl: Duration,
    /// How often to check for expired tokens
    check_interval: Duration,
}

impl TokenRefresher {
    pub fn new(
        pool: Arc<TokenPool>,
        acquirer: TokenAcquirer,
        ttl: Duration,
        check_interval: Duration,
    ) -> Self {
        Self {
            pool,
            acquirer,
            ttl,
            check_interval,
        }
    }

    /// Start the background refresh task
    pub async fn run(self) {
        info!(
            "Starting token refresher (TTL: {:?}, check interval: {:?})",
            self.ttl, self.check_interval
        );

        let mut ticker = interval(self.check_interval);
        let notify = self.pool.refresh_notify();

        loop {
            tokio::select! {
                // Periodic check for expired tokens
                _ = ticker.tick() => {
                    self.refresh_expired_tokens().await;
                }
                // Immediate refresh when notified (e.g., 401 error)
                _ = notify.notified() => {
                    self.refresh_marked_tokens().await;
                }
            }
        }
    }

    /// Refresh tokens that are expired based on TTL
    async fn refresh_expired_tokens(&self) {
        let expired = self.pool.get_expired_tokens(self.ttl);
        if expired.is_empty() {
            debug!("No expired tokens to refresh");
            return;
        }

        info!("Found {} expired tokens to refresh", expired.len());

        for token_id in expired {
            self.refresh_token(token_id).await;
        }
    }

    /// Refresh tokens that are marked as needing refresh
    async fn refresh_marked_tokens(&self) {
        let marked = self.pool.get_tokens_needing_refresh();
        if marked.is_empty() {
            return;
        }

        info!("Found {} tokens marked for refresh", marked.len());

        for token_id in marked {
            self.refresh_token(token_id).await;
        }
    }

    /// Refresh a single token
    async fn refresh_token(&self, token_id: usize) {
        let credential = match self.pool.get_credential(token_id) {
            Some(c) => c,
            None => {
                warn!("No credential found for token #{}", token_id);
                return;
            }
        };

        // Try to refresh with timeout
        match timeout(Duration::from_secs(30), self.acquirer.refresh(&credential)).await {
            Ok(Ok(new_token)) => {
                self.pool.update_token(token_id, new_token);
                info!("Successfully refreshed token #{}", token_id);
            }
            Ok(Err(e)) => {
                error!("Failed to refresh token #{}: {}", token_id, e);
            }
            Err(_) => {
                error!("Timeout refreshing token #{}", token_id);
            }
        }
    }
}

/// Spawn the token refresher as a background task
pub fn spawn_refresher(
    pool: Arc<TokenPool>,
    acquirer: TokenAcquirer,
    ttl: Duration,
    check_interval: Duration,
) -> tokio::task::JoinHandle<()> {
    let refresher = TokenRefresher::new(pool, acquirer, ttl, check_interval);
    tokio::spawn(async move {
        refresher.run().await;
    })
}
