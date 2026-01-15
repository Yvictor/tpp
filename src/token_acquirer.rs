use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

use crate::config::Credential;
use crate::error::{Result, TppError};

/// DolphinDB login request body
#[derive(Debug, Serialize)]
struct LoginRequest {
    username: String,
    password: String,
}

/// DolphinDB login response
/// See: https://docs.dolphindb.com/en/API/rest_api.html
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct LoginResponse {
    /// Session identifier
    session: Option<String>,
    /// Authenticated username
    user: Option<String>,
    /// "0" for success, "1" for failure
    code: Option<String>,
    /// Error description (empty on success)
    message: Option<String>,
    /// Array containing the user token on success
    result: Option<Vec<String>>,
}

/// Acquires tokens from DolphinDB by calling the login API
#[derive(Clone)]
pub struct TokenAcquirer {
    client: Client,
    login_url: String,
}

impl TokenAcquirer {
    /// Create a new token acquirer
    pub fn new(base_url: &str) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        let login_url = format!("{}/api/login", base_url);

        Self { client, login_url }
    }

    /// Login with a single credential and return the token
    pub async fn login(&self, credential: &Credential) -> Result<String> {
        let request = LoginRequest {
            username: credential.username.clone(),
            password: credential.password.clone(),
        };

        let response = self
            .client
            .post(&self.login_url)
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                TppError::TokenPool(format!(
                    "Failed to send login request for user '{}': {}",
                    credential.username, e
                ))
            })?;

        if !response.status().is_success() {
            return Err(TppError::TokenPool(format!(
                "Login failed for user '{}': HTTP {}",
                credential.username,
                response.status()
            )));
        }

        let login_response: LoginResponse = response.json().await.map_err(|e| {
            TppError::TokenPool(format!(
                "Failed to parse login response for user '{}': {}",
                credential.username, e
            ))
        })?;

        // Check result code ("0" = success, "1" = failure in DolphinDB)
        if let Some(code) = &login_response.code {
            if code != "0" {
                let msg = login_response
                    .message
                    .clone()
                    .unwrap_or_else(|| "Unknown error".to_string());
                return Err(TppError::TokenPool(format!(
                    "Login failed for user '{}': {} (code: {})",
                    credential.username, msg, code
                )));
            }
        }

        // Extract token from result array
        login_response
            .result
            .and_then(|r| r.into_iter().next())
            .ok_or_else(|| {
                TppError::TokenPool(format!(
                    "Login response for user '{}' missing token in result",
                    credential.username
                ))
            })
    }

    /// Acquire N tokens using a single credential
    /// Returns a list of token strings
    pub async fn acquire_n(&self, credential: &Credential, count: usize) -> Result<Vec<String>> {
        info!(
            "Acquiring {} tokens from DolphinDB for user '{}'...",
            count, credential.username
        );

        let mut tokens = Vec::with_capacity(count);
        let mut failures = 0;

        for i in 0..count {
            match self.login(credential).await {
                Ok(token) => {
                    tokens.push(token);
                    if (i + 1) % 10 == 0 || i + 1 == count {
                        info!("Acquired {}/{} tokens", i + 1, count);
                    }
                }
                Err(e) => {
                    failures += 1;
                    error!("Failed to acquire token ({}/{}): {}", i + 1, count, e);
                    // Continue trying to acquire remaining tokens
                }
            }
        }

        if failures > 0 {
            warn!(
                "Token acquisition completed with {} failures ({}/{} successful)",
                failures,
                tokens.len(),
                count
            );
        } else {
            info!("Successfully acquired all {} tokens", tokens.len());
        }

        if tokens.is_empty() {
            return Err(TppError::TokenPool(
                "Failed to acquire any tokens. Check credentials and DolphinDB connectivity."
                    .to_string(),
            ));
        }

        Ok(tokens)
    }

    /// Refresh a single token
    pub async fn refresh(&self, credential: &Credential) -> Result<String> {
        info!("Refreshing token for user '{}'", credential.username);
        self.login(credential).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_login_url() {
        let acquirer = TokenAcquirer::new("http://localhost:8848");
        assert_eq!(acquirer.login_url, "http://localhost:8848/api/login");
    }
}
