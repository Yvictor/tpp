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

        let mut config: Config = serde_yaml::from_str(&content)?;
        config.apply_env_overrides();
        config.validate()?;
        Ok(config)
    }

    /// Create configuration purely from environment variables
    pub fn from_env() -> Result<Self> {
        let config = Self {
            listen: std::env::var("TPP_LISTEN").unwrap_or_else(|_| "0.0.0.0:8080".to_string()),
            health_listen: std::env::var("TPP_HEALTH_LISTEN").ok(),
            upstream: UpstreamConfig {
                host: std::env::var("TPP_UPSTREAM_HOST").unwrap_or_default(),
                port: std::env::var("TPP_UPSTREAM_PORT")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(8848),
                tls: std::env::var("TPP_UPSTREAM_TLS")
                    .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
                    .unwrap_or(false),
            },
            credential: Credential {
                username: std::env::var("TPP_CREDENTIAL_USERNAME").unwrap_or_default(),
                password: std::env::var("TPP_CREDENTIAL_PASSWORD").unwrap_or_default(),
            },
            token: TokenConfig {
                pool_size: std::env::var("TPP_TOKEN_POOL_SIZE")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or_else(default_pool_size),
                ttl_seconds: std::env::var("TPP_TOKEN_TTL_SECONDS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or_else(default_token_ttl),
                refresh_check_seconds: std::env::var("TPP_TOKEN_REFRESH_CHECK_SECONDS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or_else(default_refresh_interval),
            },
            telemetry: TelemetryConfig {
                otlp_endpoint: std::env::var("TPP_TELEMETRY_OTLP_ENDPOINT").ok(),
                log_filter: std::env::var("TPP_TELEMETRY_LOG_FILTER").ok(),
            },
        };
        config.validate()?;
        Ok(config)
    }

    /// Apply environment variable overrides
    /// Environment variables take precedence over config file values
    fn apply_env_overrides(&mut self) {
        // Listen address
        if let Ok(val) = std::env::var("TPP_LISTEN") {
            self.listen = val;
        }

        // Health listen address
        if let Ok(val) = std::env::var("TPP_HEALTH_LISTEN") {
            self.health_listen = Some(val);
        }

        // Upstream settings
        if let Ok(val) = std::env::var("TPP_UPSTREAM_HOST") {
            self.upstream.host = val;
        }
        if let Ok(val) = std::env::var("TPP_UPSTREAM_PORT") {
            if let Ok(port) = val.parse() {
                self.upstream.port = port;
            }
        }
        if let Ok(val) = std::env::var("TPP_UPSTREAM_TLS") {
            self.upstream.tls = val.eq_ignore_ascii_case("true") || val == "1";
        }

        // Credential settings
        if let Ok(val) = std::env::var("TPP_CREDENTIAL_USERNAME") {
            self.credential.username = val;
        }
        if let Ok(val) = std::env::var("TPP_CREDENTIAL_PASSWORD") {
            self.credential.password = val;
        }

        // Token settings
        if let Ok(val) = std::env::var("TPP_TOKEN_POOL_SIZE") {
            if let Ok(size) = val.parse() {
                self.token.pool_size = size;
            }
        }
        if let Ok(val) = std::env::var("TPP_TOKEN_TTL_SECONDS") {
            if let Ok(ttl) = val.parse() {
                self.token.ttl_seconds = ttl;
            }
        }
        if let Ok(val) = std::env::var("TPP_TOKEN_REFRESH_CHECK_SECONDS") {
            if let Ok(interval) = val.parse() {
                self.token.refresh_check_seconds = interval;
            }
        }

        // Telemetry settings
        if let Ok(val) = std::env::var("TPP_TELEMETRY_OTLP_ENDPOINT") {
            self.telemetry.otlp_endpoint = Some(val);
        }
        if let Ok(val) = std::env::var("TPP_TELEMETRY_LOG_FILTER") {
            self.telemetry.log_filter = Some(val);
        }
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
            return Err(TppError::Config(
                "'credential.username' is required".to_string(),
            ));
        }

        if self.token.pool_size == 0 {
            return Err(TppError::Config(
                "'token.pool_size' must be > 0".to_string(),
            ));
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
