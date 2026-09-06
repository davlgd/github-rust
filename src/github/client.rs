use crate::{config::*, error::*};
use reqwest::{Client, Url, header::HeaderValue};
use secrecy::{ExposeSecret, SecretString};
use std::env;

/// Low-level GitHub API client with connection pooling.
///
/// Handles HTTP requests, authentication, and rate limiting.
/// For most use cases, prefer using [`GitHubService`](crate::GitHubService) instead.
#[derive(Clone)]
pub struct GitHubClient {
    client: Client,
    rest_url: String,
    graphql_url: String,
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
        let mut builder = Self::builder();
        if let Ok(token) = env::var("GITHUB_TOKEN") {
            builder = builder.token(SecretString::from(token));
        }
        builder.build()
    }

    /// Creates a builder with no token; environment variables are not read.
    pub fn builder() -> GitHubClientBuilder {
        GitHubClientBuilder::default()
    }

    pub(crate) fn rest_url(&self) -> &str {
        &self.rest_url
    }

    pub(crate) fn graphql_url(&self) -> &str {
        &self.graphql_url
    }

    pub(crate) fn get(&self, url: impl reqwest::IntoUrl) -> reqwest::RequestBuilder {
        self.request(reqwest::Method::GET, url)
    }

    pub(crate) fn post(&self, url: impl reqwest::IntoUrl) -> reqwest::RequestBuilder {
        self.request(reqwest::Method::POST, url)
    }

    fn request(
        &self,
        method: reqwest::Method,
        url: impl reqwest::IntoUrl,
    ) -> reqwest::RequestBuilder {
        let request = self
            .client
            .request(method, url)
            .header("User-Agent", USER_AGENT)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28");
        match &self.token {
            Some(token) => request.bearer_auth(token.expose_secret()),
            None => request,
        }
    }

    #[must_use]
    pub fn has_token(&self) -> bool {
        self.token.is_some()
    }

    /// Raw HTTP transport. Library authentication headers are added only by API methods.
    #[must_use]
    pub fn client(&self) -> &Client {
        &self.client
    }

    pub async fn check_rate_limit(&self) -> Result<RateLimit> {
        let response = self
            .get(format!("{}/rate_limit", self.rest_url()))
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

/// Explicit configuration for authentication, endpoints and HTTP transport.
///
/// Custom endpoints receive the configured token. Only use endpoints you trust.
/// An injected HTTP client controls timeouts, proxies and redirects.
pub struct GitHubClientBuilder {
    client: Option<Client>,
    token: Option<SecretString>,
    rest_url: String,
    graphql_url: String,
}

impl Default for GitHubClientBuilder {
    fn default() -> Self {
        Self {
            client: None,
            token: None,
            rest_url: GITHUB_API_URL.to_owned(),
            graphql_url: GITHUB_GRAPHQL_URL.to_owned(),
        }
    }
}

impl GitHubClientBuilder {
    pub fn token(mut self, token: SecretString) -> Self {
        self.token = Some(token);
        self
    }

    pub fn http_client(mut self, client: Client) -> Self {
        self.client = Some(client);
        self
    }

    pub fn rest_url(mut self, url: impl Into<String>) -> Self {
        self.rest_url = url.into();
        self
    }

    pub fn graphql_url(mut self, url: impl Into<String>) -> Self {
        self.graphql_url = url.into();
        self
    }

    pub fn build(self) -> Result<GitHubClient> {
        for endpoint in [&self.rest_url, &self.graphql_url] {
            let url = Url::parse(endpoint)
                .map_err(|_| GitHubError::ConfigError("Invalid API endpoint URL".into()))?;
            if !matches!(url.scheme(), "http" | "https")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err(GitHubError::ConfigError(
                    "API endpoints must be HTTP(S) URLs without credentials, query or fragment"
                        .into(),
                ));
            }
        }
        if let Some(token) = &self.token {
            if token.expose_secret().trim().is_empty() {
                return Err(GitHubError::ConfigError(
                    "GitHub token cannot be empty".into(),
                ));
            }
            HeaderValue::from_str(&format!("Bearer {}", token.expose_secret()))
                .map_err(|_| GitHubError::ConfigError("Invalid Authorization header".into()))?;
        }
        let client = match self.client {
            Some(client) => client,
            None => Client::builder().timeout(DEFAULT_TIMEOUT).build()?,
        };
        Ok(GitHubClient {
            client,
            token: self.token,
            rest_url: self.rest_url.trim_end_matches('/').to_owned(),
            graphql_url: self.graphql_url,
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
