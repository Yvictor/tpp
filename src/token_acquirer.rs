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
#[derive(Debug, Deserialize)]
struct LoginResponse {
    #[serde(rename = "userToken")]
    user_token: Option<String>,
    #[serde(rename = "resultCode")]
    result_code: Option<i32>,
    #[serde(rename = "msg")]
    message: Option<String>,
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

        // Check result code (0 = success in DolphinDB)
        if let Some(code) = login_response.result_code {
            if code != 0 {
                let msg = login_response
                    .message
                    .unwrap_or_else(|| "Unknown error".to_string());
                return Err(TppError::TokenPool(format!(
                    "Login failed for user '{}': {} (code: {})",
                    credential.username, msg, code
                )));
            }
        }

        login_response.user_token.ok_or_else(|| {
            TppError::TokenPool(format!(
                "Login response for user '{}' missing userToken",
                credential.username
            ))
        })
    }

    /// Acquire tokens for all credentials
    /// Returns a list of (token, credential) pairs
    pub async fn acquire_all(
        &self,
        credentials: &[Credential],
    ) -> Result<Vec<(String, Credential)>> {
        let total = credentials.len();
        info!("Acquiring {} tokens from DolphinDB...", total);

        let mut results = Vec::with_capacity(total);
        let mut failures = 0;

        for (i, cred) in credentials.iter().enumerate() {
            match self.login(cred).await {
                Ok(token) => {
                    results.push((token, cred.clone()));
                    if (i + 1) % 10 == 0 || i + 1 == total {
                        info!("Acquired {}/{} tokens", i + 1, total);
                    }
                }
                Err(e) => {
                    failures += 1;
                    error!(
                        "Failed to acquire token for user '{}' ({}/{}): {}",
                        cred.username,
                        i + 1,
                        total,
                        e
                    );
                }
            }
        }

        if failures > 0 {
            warn!(
                "Token acquisition completed with {} failures ({}/{} successful)",
                failures,
                results.len(),
                total
            );
        } else {
            info!("Successfully acquired all {} tokens", results.len());
        }

        if results.is_empty() {
            return Err(TppError::TokenPool(
                "Failed to acquire any tokens. Check credentials and DolphinDB connectivity."
                    .to_string(),
            ));
        }

        Ok(results)
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
