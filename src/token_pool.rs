use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_channel::{Receiver, Sender};
use dashmap::DashMap;
use parking_lot::RwLock;
use tokio::sync::Notify;
use tracing::{debug, info, warn};

use crate::config::Credential;

/// A single token in the pool
#[derive(Clone, Debug)]
pub struct Token {
    /// The actual bearer token value
    pub value: String,
    /// Unique ID for this token (0-indexed)
    pub id: usize,
}

/// Token metadata including expiration and credential info
pub struct TokenMeta {
    /// Current token value
    pub value: RwLock<String>,
    /// When this token was acquired
    pub acquired_at: RwLock<Instant>,
    /// The credential used to acquire this token
    pub credential: Credential,
    /// Number of times this token has been used
    pub use_count: AtomicU64,
    /// Number of errors encountered with this token
    pub error_count: AtomicU64,
    /// Last time this token was used (unix timestamp)
    pub last_used: AtomicU64,
    /// Whether this token needs refresh
    pub needs_refresh: AtomicU64, // 0 = no, 1 = yes
}

impl TokenMeta {
    fn new(value: String, credential: Credential) -> Self {
        Self {
            value: RwLock::new(value),
            acquired_at: RwLock::new(Instant::now()),
            credential,
            use_count: AtomicU64::new(0),
            error_count: AtomicU64::new(0),
            last_used: AtomicU64::new(0),
            needs_refresh: AtomicU64::new(0),
        }
    }

    fn record_use(&self) {
        self.use_count.fetch_add(1, Ordering::Relaxed);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.last_used.store(now, Ordering::Relaxed);
    }

    fn record_error(&self) {
        self.error_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Check if token is older than the given duration
    pub fn is_expired(&self, ttl: Duration) -> bool {
        self.acquired_at.read().elapsed() > ttl
    }

    /// Mark token as needing refresh
    pub fn mark_needs_refresh(&self) {
        self.needs_refresh.store(1, Ordering::Relaxed);
    }

    /// Check if token needs refresh
    pub fn needs_refresh(&self) -> bool {
        self.needs_refresh.load(Ordering::Relaxed) == 1
    }

    /// Update token value after refresh
    pub fn update(&self, new_value: String) {
        *self.value.write() = new_value;
        *self.acquired_at.write() = Instant::now();
        self.needs_refresh.store(0, Ordering::Relaxed);
    }

    /// Get current token value
    pub fn get_value(&self) -> String {
        self.value.read().clone()
    }
}

/// Token pool with semaphore-like semantics using async channels
pub struct TokenPool {
    /// Channel to receive available tokens (just IDs)
    available_rx: Receiver<usize>,
    /// Channel to return tokens
    return_tx: Sender<usize>,
    /// Total number of tokens in the pool
    total_count: usize,
    /// Number of tokens currently in use
    in_use: AtomicU64,
    /// Number of requests waiting for a token
    waiting: AtomicU64,
    /// Token metadata (id -> metadata)
    token_meta: DashMap<usize, TokenMeta>,
    /// Notify for refresh task
    refresh_notify: Arc<Notify>,
}

impl TokenPool {
    /// Create a new token pool from tokens and their credentials
    pub fn new(tokens_with_creds: Vec<(String, Credential)>) -> Arc<Self> {
        let total_count = tokens_with_creds.len();
        info!("Creating token pool with {} tokens", total_count);

        // Create bounded channel with capacity = number of tokens
        let (tx, rx) = async_channel::bounded(total_count);

        // Initialize metadata and populate channel with token IDs
        let token_meta = DashMap::new();
        for (id, (value, credential)) in tokens_with_creds.into_iter().enumerate() {
            token_meta.insert(id, TokenMeta::new(value, credential));
            // Send token ID to channel
            tx.try_send(id).expect("Channel should have capacity");
        }

        Arc::new(Self {
            available_rx: rx,
            return_tx: tx,
            total_count,
            in_use: AtomicU64::new(0),
            waiting: AtomicU64::new(0),
            token_meta,
            refresh_notify: Arc::new(Notify::new()),
        })
    }

    /// Acquire a token from the pool, waiting indefinitely if none available
    pub async fn acquire(&self) -> Token {
        // Increment waiting counter
        self.waiting.fetch_add(1, Ordering::Relaxed);

        debug!(
            "Waiting for token (in_use: {}, waiting: {})",
            self.in_use.load(Ordering::Relaxed),
            self.waiting.load(Ordering::Relaxed)
        );

        // Wait for a token ID (blocks if pool is exhausted)
        let token_id = self.available_rx.recv().await.expect("Channel closed unexpectedly");

        // Update counters
        self.waiting.fetch_sub(1, Ordering::Relaxed);
        self.in_use.fetch_add(1, Ordering::Relaxed);

        // Get token value and record usage
        let value = if let Some(meta) = self.token_meta.get(&token_id) {
            meta.record_use();
            meta.get_value()
        } else {
            String::new()
        };

        debug!(
            "Acquired token #{} (in_use: {}, available: {})",
            token_id,
            self.in_use.load(Ordering::Relaxed),
            self.available()
        );

        Token { value, id: token_id }
    }

    /// Release a token back to the pool
    pub fn release(&self, token: Token) {
        let token_id = token.id;

        // Decrement in_use counter
        self.in_use.fetch_sub(1, Ordering::Relaxed);

        // Return token ID to the channel
        if let Err(e) = self.return_tx.try_send(token_id) {
            warn!("Failed to return token #{}: {}", token_id, e);
        } else {
            debug!(
                "Released token #{} (in_use: {}, available: {})",
                token_id,
                self.in_use.load(Ordering::Relaxed),
                self.available()
            );
        }
    }

    /// Mark that a token encountered an error (possibly needs refresh)
    pub fn mark_error(&self, token: &Token) {
        if let Some(meta) = self.token_meta.get(&token.id) {
            meta.record_error();
            warn!(
                "Token #{} error count: {}",
                token.id,
                meta.error_count.load(Ordering::Relaxed)
            );
        }
    }

    /// Mark token as needing refresh (e.g., got 401)
    pub fn mark_needs_refresh(&self, token_id: usize) {
        if let Some(meta) = self.token_meta.get(&token_id) {
            meta.mark_needs_refresh();
            info!("Token #{} marked for refresh", token_id);
            self.refresh_notify.notify_one();
        }
    }

    /// Update a token's value after refresh
    pub fn update_token(&self, token_id: usize, new_value: String) {
        if let Some(meta) = self.token_meta.get(&token_id) {
            meta.update(new_value);
            info!("Token #{} refreshed", token_id);
        }
    }

    /// Get credential for a token (for refresh)
    pub fn get_credential(&self, token_id: usize) -> Option<Credential> {
        self.token_meta.get(&token_id).map(|m| m.credential.clone())
    }

    /// Get tokens that need refresh
    pub fn get_tokens_needing_refresh(&self) -> Vec<usize> {
        self.token_meta
            .iter()
            .filter(|entry| entry.value().needs_refresh())
            .map(|entry| *entry.key())
            .collect()
    }

    /// Get tokens that are expired based on TTL
    pub fn get_expired_tokens(&self, ttl: Duration) -> Vec<usize> {
        self.token_meta
            .iter()
            .filter(|entry| entry.value().is_expired(ttl))
            .map(|entry| *entry.key())
            .collect()
    }

    /// Get refresh notification handle
    pub fn refresh_notify(&self) -> Arc<Notify> {
        self.refresh_notify.clone()
    }

    /// Get total number of tokens in the pool
    pub fn total(&self) -> usize {
        self.total_count
    }

    /// Get number of tokens currently in use
    pub fn in_use(&self) -> u64 {
        self.in_use.load(Ordering::Relaxed)
    }

    /// Get number of available tokens
    pub fn available(&self) -> usize {
        self.available_rx.len()
    }

    /// Get number of requests waiting for a token
    pub fn waiting(&self) -> u64 {
        self.waiting.load(Ordering::Relaxed)
    }

    /// Get statistics for a specific token
    pub fn get_token_stats(&self, id: usize) -> Option<(u64, u64, u64)> {
        self.token_meta.get(&id).map(|meta| {
            (
                meta.use_count.load(Ordering::Relaxed),
                meta.error_count.load(Ordering::Relaxed),
                meta.last_used.load(Ordering::Relaxed),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_cred(name: &str) -> Credential {
        Credential {
            username: name.to_string(),
            password: "pass".to_string(),
        }
    }

    #[tokio::test]
    async fn test_acquire_release() {
        let pool = TokenPool::new(vec![
            ("token1".to_string(), make_cred("user1")),
            ("token2".to_string(), make_cred("user2")),
        ]);

        assert_eq!(pool.total(), 2);
        assert_eq!(pool.available(), 2);
        assert_eq!(pool.in_use(), 0);

        let t1 = pool.acquire().await;
        assert_eq!(pool.available(), 1);
        assert_eq!(pool.in_use(), 1);

        let t2 = pool.acquire().await;
        assert_eq!(pool.available(), 0);
        assert_eq!(pool.in_use(), 2);

        pool.release(t1);
        assert_eq!(pool.available(), 1);
        assert_eq!(pool.in_use(), 1);

        pool.release(t2);
        assert_eq!(pool.available(), 2);
        assert_eq!(pool.in_use(), 0);
    }

    #[tokio::test]
    async fn test_token_refresh() {
        let pool = TokenPool::new(vec![("old_token".to_string(), make_cred("user1"))]);

        let t = pool.acquire().await;
        assert_eq!(t.value, "old_token");

        // Mark for refresh and update
        pool.mark_needs_refresh(t.id);
        assert!(pool.get_tokens_needing_refresh().contains(&t.id));

        pool.update_token(t.id, "new_token".to_string());
        assert!(pool.get_tokens_needing_refresh().is_empty());

        pool.release(t);

        // Acquire again should get new value
        let t2 = pool.acquire().await;
        assert_eq!(t2.value, "new_token");
    }
}
