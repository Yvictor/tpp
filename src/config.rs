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
    /// Token TTL in seconds (default: 3600 = 1 hour)
    #[serde(default = "default_token_ttl")]
    pub ttl_seconds: u64,

    /// How often to check for expired tokens in seconds (default: 60)
    #[serde(default = "default_refresh_interval")]
    pub refresh_check_seconds: u64,
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

    /// User credentials for automatic token acquisition
    #[serde(default)]
    pub credentials: Vec<Credential>,

    /// Path to file containing credentials (format: username:password per line)
    pub credentials_file: Option<String>,

    /// Token refresh configuration
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

    /// Get all credentials (from inline config and/or credentials file)
    pub fn load_credentials(&self) -> Result<Vec<Credential>> {
        let mut credentials = self.credentials.clone();

        // Load credentials from file if specified
        if let Some(ref file_path) = self.credentials_file {
            let content = fs::read_to_string(file_path).map_err(|e| {
                TppError::Config(format!(
                    "Failed to read credentials file '{}': {}",
                    file_path, e
                ))
            })?;

            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }

                // Format: username:password
                let parts: Vec<&str> = line.splitn(2, ':').collect();
                if parts.len() != 2 {
                    return Err(TppError::Config(format!(
                        "Invalid credential format in file: '{}'. Expected 'username:password'",
                        line
                    )));
                }

                credentials.push(Credential {
                    username: parts[0].to_string(),
                    password: parts[1].to_string(),
                });
            }
        }

        if credentials.is_empty() {
            return Err(TppError::Config(
                "No credentials configured. Provide 'credentials' or 'credentials_file' in config."
                    .to_string(),
            ));
        }

        Ok(credentials)
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

        // Check that at least one credential source is provided
        if self.credentials.is_empty() && self.credentials_file.is_none() {
            return Err(TppError::Config(
                "Either 'credentials' or 'credentials_file' must be provided".to_string(),
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

credentials:
  - username: "user1"
    password: "pass1"
  - username: "user2"
    password: "pass2"

telemetry:
  otlp_endpoint: "http://localhost:4317"
"#;

        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.listen, "0.0.0.0:8080");
        assert_eq!(config.upstream.host, "dolphindb.example.com");
        assert_eq!(config.upstream.port, 8848);
        assert!(!config.upstream.tls);
        assert_eq!(config.credentials.len(), 2);
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
