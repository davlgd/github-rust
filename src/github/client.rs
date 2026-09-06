use crate::{config::*, error::*};
use reqwest::{Client, Url, header::HeaderValue};
use secrecy::{ExposeSecret, SecretString};
use std::env;

/// Low-level GitHub API client with connection pooling.
///
/// Configures HTTP requests and authentication, and retrieves quota information.
/// Requests are not automatically delayed or retried when quotas are exhausted.
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
    /// Reads `GITHUB_TOKEN` from the environment. An unset, empty or whitespace-only
    /// variable produces an anonymous client; the builder itself still rejects empty tokens.
    /// The token is stored securely using [`SecretString`] and is automatically
    /// zeroized when the client is dropped.
    pub fn new() -> Result<Self> {
        let mut builder = Self::builder();
        if let Some(token) = env::var("GITHUB_TOKEN")
            .ok()
            .filter(|token| !token.trim().is_empty())
        {
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

    /// Returns the REST core quota. Use `check_rate_limits` for GraphQL and search quotas.
    pub async fn check_rate_limit(&self) -> Result<RateLimit> {
        self.check_rate_limits()
            .await?
            .resources
            .remove("core")
            .ok_or_else(|| GitHubError::ParseError("Missing core rate limit resource".into()))
    }

    /// Retrieves separate quotas for every resource returned by GitHub.
    pub async fn check_rate_limits(&self) -> Result<RateLimits> {
        let response = self
            .get(format!("{}/rate_limit", self.rest_url()))
            .send()
            .await?;
        Ok(super::response::check(response).await?.json().await?)
    }
}

/// Quotas indexed by GitHub resource names such as `core`, `search` and `graphql`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct RateLimits {
    pub resources: std::collections::BTreeMap<String, RateLimit>,
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
    /// Set the token, for example `.token("your-token".into())`.
    /// Empty tokens and invalid authorization header values are rejected by `build()`.
    pub fn token(mut self, token: SecretString) -> Self {
        self.token = Some(token);
        self
    }

    /// Use this transport's timeout, proxy and redirect policy instead of the defaults.
    pub fn http_client(mut self, client: Client) -> Self {
        self.client = Some(client);
        self
    }

    /// Set the REST base URL, including any Enterprise API prefix.
    /// Configure `graphql_url` separately when using a custom installation.
    pub fn rest_url(mut self, url: impl Into<String>) -> Self {
        self.rest_url = url.into();
        self
    }

    /// Set the complete GraphQL endpoint URL. It receives the configured token.
    pub fn graphql_url(mut self, url: impl Into<String>) -> Self {
        self.graphql_url = url.into();
        self
    }

    /// Validate endpoints and token, then create the client.
    /// The default transport uses a 30-second request timeout.
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
            // REST paths are appended to this base; the GraphQL URL is posted to as given.
            rest_url: self.rest_url.trim_end_matches('/').to_owned(),
            graphql_url: self.graphql_url,
        })
    }
}

/// GitHub API rate limit information.
///
/// Provides information about API usage limits and reset times.
/// Quotas depend on the API resource and authentication type.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct RateLimit {
    /// Maximum requests or points allowed in the current resource-specific window
    pub limit: u64,
    /// Number of requests remaining in the current window
    pub remaining: u64,
    /// Unix timestamp when the rate limit resets
    pub reset: u64,
}

impl RateLimit {
    /// Returns the reset datetime, or None if the timestamp cannot be represented.
    #[must_use]
    pub fn reset_datetime(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        i64::try_from(self.reset)
            .ok()
            .and_then(|reset| chrono::DateTime::from_timestamp(reset, 0))
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
