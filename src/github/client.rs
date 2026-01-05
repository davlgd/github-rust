use crate::{config::*, error::*};
use reqwest::{Client, header::HeaderMap};
use secrecy::{ExposeSecret, SecretString};
use std::env;

/// Low-level GitHub API client with connection pooling.
///
/// Handles HTTP requests, authentication, and rate limiting.
/// For most use cases, prefer using [`GitHubService`](crate::GitHubService) instead.
#[derive(Clone)]
pub struct GitHubClient {
    client: Client,
    /// Token stored securely - automatically zeroized on drop
    token: Option<SecretString>,
}

impl GitHubClient {
    /// Creates a new GitHub API client with connection pooling and optional token authentication.
    ///
    /// Automatically detects `GITHUB_TOKEN` from environment variables.
    /// The token is stored securely using [`SecretString`] and is automatically
    /// zeroized when the client is dropped.
    #[must_use = "Creating a client without using it is wasteful"]
    pub fn new() -> Result<Self> {
        let token: Option<SecretString> = env::var("GITHUB_TOKEN").ok().map(SecretString::from);
        let mut headers = HeaderMap::new();

        headers.insert(
            "User-Agent",
            USER_AGENT
                .parse()
                .map_err(|_| GitHubError::ConfigError("Invalid User-Agent header".to_string()))?,
        );
        headers.insert(
            "Accept",
            "application/vnd.github+json"
                .parse()
                .map_err(|_| GitHubError::ConfigError("Invalid Accept header".to_string()))?,
        );
        headers.insert(
            "X-GitHub-Api-Version",
            "2022-11-28"
                .parse()
                .map_err(|_| GitHubError::ConfigError("Invalid API version header".to_string()))?,
        );

        if let Some(ref token) = token {
            let auth_value = format!("Bearer {}", token.expose_secret());
            headers.insert(
                "Authorization",
                auth_value.parse().map_err(|_| {
                    GitHubError::ConfigError("Invalid Authorization header".to_string())
                })?,
            );
        }

        let client = Client::builder()
            .timeout(DEFAULT_TIMEOUT)
            .default_headers(headers)
            .build()
            .map_err(|e| GitHubError::NetworkError(e.to_string()))?;

        Ok(Self { client, token })
    }

    #[must_use]
    pub fn has_token(&self) -> bool {
        self.token.is_some()
    }

    #[must_use]
    pub fn client(&self) -> &Client {
        &self.client
    }

    pub async fn check_rate_limit(&self) -> Result<RateLimit> {
        let response = self
            .client
            .get(format!("{}/rate_limit", GITHUB_API_URL))
            .send()
            .await?;

        if response.status() == 403 {
            // For rate_limit endpoint, 403 should always be actual rate limiting
            // But let's be defensive and check the response content
            let error_text = response.text().await.unwrap_or_default();
            let error_lower = error_text.to_lowercase();

            if error_lower.contains("rate limit") || error_lower.is_empty() {
                // Empty response or explicit rate limit message
                return Err(GitHubError::RateLimitError(
                    "API rate limit exceeded".to_string(),
                ));
            } else if error_lower.contains("repository access blocked")
                || error_lower.contains("access blocked")
            {
                return Err(GitHubError::AccessBlockedError(
                    "Rate limit check blocked".to_string(),
                ));
            } else {
                return Err(GitHubError::AuthenticationError(format!(
                    "Access denied for rate limit check: {}",
                    error_text
                )));
            }
        }

        let rate_limit_response: serde_json::Value = response.json().await?;
        let rate = &rate_limit_response["rate"];

        Ok(RateLimit {
            limit: rate["limit"].as_u64().unwrap_or(0),
            remaining: rate["remaining"].as_u64().unwrap_or(0),
            reset: rate["reset"].as_u64().unwrap_or(0),
        })
    }
}

/// GitHub API rate limit information.
///
/// Provides information about API usage limits and reset times.
/// Authenticated requests have a limit of 5000/hour, unauthenticated 60/hour.
#[derive(Debug, Clone)]
pub struct RateLimit {
    /// Maximum number of requests allowed per hour
    pub limit: u64,
    /// Number of requests remaining in the current window
    pub remaining: u64,
    /// Unix timestamp when the rate limit resets
    pub reset: u64,
}

impl RateLimit {
    /// Returns the datetime when the rate limit resets.
    #[must_use]
    pub fn reset_datetime(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::from_timestamp(self.reset as i64, 0).unwrap_or_else(chrono::Utc::now)
    }

    /// Returns the duration until the rate limit resets.
    #[must_use]
    pub fn time_until_reset(&self) -> std::time::Duration {
        let now = chrono::Utc::now().timestamp() as u64;
        if self.reset > now {
            std::time::Duration::from_secs(self.reset - now)
        } else {
            std::time::Duration::ZERO
        }
    }

    /// Returns true if the rate limit has been exceeded (no requests remaining).
    #[must_use]
    pub fn is_exceeded(&self) -> bool {
        self.remaining == 0
    }

    /// Returns the number of requests used in the current window.
    #[must_use]
    pub fn used(&self) -> u64 {
        self.limit.saturating_sub(self.remaining)
    }
}
