use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::error::{Result, TppError};

/// User credential for DolphinDB login
#[derive(Debug, Deserialize, Clone)]
pub struct Credential {
    pub username: String,
    pub password: String,
}

/// Upstream server configuration
#[derive(Debug, Deserialize, Clone)]
pub struct UpstreamConfig {
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub tls: bool,
}

impl UpstreamConfig {
    /// Get the upstream address as "host:port"
    pub fn address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    /// Get the base URL for API calls
    pub fn base_url(&self) -> String {
        let scheme = if self.tls { "https" } else { "http" };
        format!("{}://{}:{}", scheme, self.host, self.port)
    }
}

/// Telemetry configuration
#[derive(Debug, Deserialize, Clone, Default)]
pub struct TelemetryConfig {
    /// OTLP endpoint for exporting traces and metrics
    pub otlp_endpoint: Option<String>,
    /// Log filter (e.g., "info", "debug", "tpp=debug")
    pub log_filter: Option<String>,
}

/// Token refresh configuration
#[derive(Debug, Deserialize, Clone)]
pub struct TokenConfig {
    /// Number of tokens to acquire (pool size)
    #[serde(default = "default_pool_size")]
    pub pool_size: usize,

    /// Token TTL in seconds (default: 3600 = 1 hour)
    #[serde(default = "default_token_ttl")]
    pub ttl_seconds: u64,

    /// How often to check for expired tokens in seconds (default: 60)
    #[serde(default = "default_refresh_interval")]
    pub refresh_check_seconds: u64,
}

fn default_pool_size() -> usize {
    10
}

fn default_token_ttl() -> u64 {
    3600 // 1 hour
}

fn default_refresh_interval() -> u64 {
    60 // 1 minute
}

impl Default for TokenConfig {
    fn default() -> Self {
        Self {
            pool_size: default_pool_size(),
            ttl_seconds: default_token_ttl(),
            refresh_check_seconds: default_refresh_interval(),
        }
    }
}

/// Main configuration structure
#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    /// Listen address (e.g., "0.0.0.0:8080")
    pub listen: String,

    /// Health check server listen address (e.g., "0.0.0.0:9090")
    pub health_listen: Option<String>,

    /// Upstream server configuration
    pub upstream: UpstreamConfig,

    /// Single credential for token acquisition
    /// The same credential will be used to acquire `token.pool_size` tokens
    pub credential: Credential,

    /// Token configuration (pool size, TTL, refresh interval)
    #[serde(default)]
    pub token: TokenConfig,

    /// Telemetry configuration
    #[serde(default)]
    pub telemetry: TelemetryConfig,
}

impl Config {
    /// Load configuration from a YAML file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(path.as_ref())
            .map_err(|e| TppError::Config(format!("Failed to read config file: {}", e)))?;

        let config: Config = serde_yaml::from_str(&content)?;
        config.validate()?;
        Ok(config)
    }

    /// Validate configuration
    fn validate(&self) -> Result<()> {
        if self.listen.is_empty() {
            return Err(TppError::Config("'listen' address is required".to_string()));
        }

        if self.upstream.host.is_empty() {
            return Err(TppError::Config("'upstream.host' is required".to_string()));
        }

        if self.upstream.port == 0 {
            return Err(TppError::Config("'upstream.port' must be > 0".to_string()));
        }

        if self.credential.username.is_empty() {
            return Err(TppError::Config("'credential.username' is required".to_string()));
        }

        if self.token.pool_size == 0 {
            return Err(TppError::Config("'token.pool_size' must be > 0".to_string()));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_config() {
        let yaml = r#"
listen: "0.0.0.0:8080"

upstream:
  host: "dolphindb.example.com"
  port: 8848
  tls: false

credential:
  username: "user1"
  password: "pass1"

token:
  pool_size: 200
  ttl_seconds: 3600

telemetry:
  otlp_endpoint: "http://localhost:4317"
"#;

        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.listen, "0.0.0.0:8080");
        assert_eq!(config.upstream.host, "dolphindb.example.com");
        assert_eq!(config.upstream.port, 8848);
        assert!(!config.upstream.tls);
        assert_eq!(config.credential.username, "user1");
        assert_eq!(config.token.pool_size, 200);
        assert_eq!(
            config.telemetry.otlp_endpoint,
            Some("http://localhost:4317".to_string())
        );
    }

    #[test]
    fn test_upstream_address() {
        let upstream = UpstreamConfig {
            host: "example.com".to_string(),
            port: 8080,
            tls: false,
        };
        assert_eq!(upstream.address(), "example.com:8080");
        assert_eq!(upstream.base_url(), "http://example.com:8080");
    }
}
