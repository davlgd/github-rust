use crate::error::*;
use crate::github::client::GitHubClient;
use crate::github::graphql::{self, Repository};
use crate::github::{rest, search};

/// REST fallback policy for authenticated repository lookups.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FallbackPolicy {
    /// Preserve every GraphQL error without trying REST.
    Never,
    /// Try REST only when GraphQL returns HTTP 502, 503 or 504.
    #[default]
    OnServerUnavailable,
}

/// High-level service for GitHub API operations.
///
/// `GitHubService` is the main entry point for interacting with the GitHub API.
/// It provides a simple, ergonomic interface with automatic fallback from GraphQL
/// to REST API when needed.
///
/// # Authentication
///
/// The service automatically detects the `GITHUB_TOKEN` environment variable.
/// Token permissions control access. Quotas vary by API resource and token type.
///
/// # Example
///
/// ```no_run
/// use github_rust::GitHubService;
///
/// # async fn example() -> github_rust::Result<()> {
/// let service = GitHubService::new()?;
///
/// // Check authentication status
/// if service.has_token() {
///     println!("Authenticated with higher rate limits");
/// }
///
/// // Get repository information
/// let repo = service.get_repository_info("rust-lang", "rust").await?;
/// println!("{} has {} stars", repo.name_with_owner, repo.stargazer_count);
/// # Ok(())
/// # }
/// ```
pub struct GitHubService {
    /// The underlying HTTP client with connection pooling.
    pub client: GitHubClient,
    fallback_policy: FallbackPolicy,
    pub(crate) fetch_options: super::FetchOptions,
}

impl GitHubService {
    /// Creates a new GitHub service with default configuration.
    ///
    /// Automatically detects `GITHUB_TOKEN` from environment variables.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be initialized (rare, typically
    /// indicates system-level TLS or network configuration issues).
    ///
    /// # Example
    ///
    /// ```no_run
    /// use github_rust::GitHubService;
    ///
    /// let service = GitHubService::new()?;
    /// # Ok::<(), github_rust::GitHubError>(())
    /// ```
    #[must_use = "Creating a service without using it is wasteful"]
    pub fn new() -> Result<Self> {
        let client = GitHubClient::new()?;
        Ok(Self::with_client(client))
    }

    /// Creates a GitHub service with a custom client.
    ///
    /// Useful for testing or when you need custom HTTP configuration.
    #[must_use]
    pub fn with_client(client: GitHubClient) -> Self {
        Self {
            client,
            fallback_policy: FallbackPolicy::default(),
            fetch_options: super::FetchOptions::default(),
        }
    }

    /// Selects which authenticated GraphQL failures may trigger REST.
    /// Anonymous repository lookups always use REST directly.
    #[must_use]
    pub fn with_fallback_policy(mut self, policy: FallbackPolicy) -> Self {
        self.fallback_policy = policy;
        self
    }

    /// Fetches detailed information about a GitHub repository.
    ///
    /// Uses REST directly without a token. Authenticated lookups use GraphQL,
    /// with REST fallback only for HTTP 502/503/504 by default.
    /// Authentication, permission, quota and decoding errors are returned unchanged.
    ///
    /// # Arguments
    ///
    /// * `owner` - Repository owner (username or organization)
    /// * `name` - Repository name
    ///
    /// # Returns
    ///
    /// Full repository details including stars, forks, language, topics, license, etc.
    ///
    /// # Errors
    ///
    /// * [`GitHubError::NotFoundError`] - Repository doesn't exist or is private without auth
    /// * [`GitHubError::RateLimitError`] - API rate limit exceeded
    /// * [`GitHubError::AccessBlockedError`] - Repository access blocked by GitHub
    /// * [`GitHubError::DmcaBlockedError`] - Repository blocked for legal reasons
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use github_rust::GitHubService;
    /// # async fn example() -> github_rust::Result<()> {
    /// let service = GitHubService::new()?;
    /// let repo = service.get_repository_info("microsoft", "vscode").await?;
    ///
    /// println!("Name: {}", repo.name_with_owner);
    /// println!("Stars: {}", repo.stargazer_count);
    /// println!("Forks: {}", repo.fork_count);
    /// if let Some(lang) = &repo.primary_language {
    ///     println!("Language: {}", lang.name);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_repository_info(&self, owner: &str, name: &str) -> Result<Repository> {
        if !self.client.has_token() {
            return rest::get_repository_info(&self.client, owner, name).await;
        }
        match graphql::get_repository_info(&self.client, owner, name).await {
            Ok(repo) => Ok(repo),
            Err(graphql_error)
                if self.fallback_policy == FallbackPolicy::OnServerUnavailable
                    && matches!(
                        &graphql_error,
                        GitHubError::ApiError {
                            status: 502..=504,
                            ..
                        }
                    ) =>
            {
                tracing::debug!("GraphQL unavailable, trying REST: {graphql_error}");
                rest::get_repository_info(&self.client, owner, name)
                    .await
                    .map_err(|rest_error| GitHubError::FallbackError {
                        graphql: Box::new(graphql_error),
                        rest: Box::new(rest_error),
                    })
            }
            Err(error) => Err(error),
        }
    }

    /// Searches for recently created repositories with filtering options.
    ///
    /// Finds repositories created within the specified time period, filtered by
    /// language and minimum star count. Results are sorted by stars (descending).
    ///
    /// # Arguments
    ///
    /// * `days_back` - Search for repos created in the last N days
    /// * `limit` - Maximum number of results (capped at 1000)
    /// * `language` - Optional programming language filter (e.g., "rust", "python", "C++")
    /// * `min_stars` - Minimum number of stars required
    ///
    /// # Errors
    ///
    /// * [`GitHubError::InvalidInput`] - Invalid language parameter
    /// * [`GitHubError::RateLimitError`] - API rate limit exceeded
    /// * [`GitHubError::AuthenticationError`] - Token required for this operation
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use github_rust::GitHubService;
    /// # async fn example() -> github_rust::Result<()> {
    /// let service = GitHubService::new()?;
    ///
    /// // Find Rust repos created in last 30 days with 50+ stars
    /// let repos = service.search_repositories(
    ///     30,           // days back
    ///     100,          // limit
    ///     Some("rust"), // language
    ///     50,           // min stars
    /// ).await?;
    ///
    /// for repo in repos {
    ///     println!("{}: {} stars", repo.name_with_owner, repo.stargazer_count);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn search_repositories(
        &self,
        days_back: u32,
        limit: usize,
        language: Option<&str>,
        min_stars: u32,
    ) -> Result<Vec<search::SearchRepository>> {
        search::search_repositories(&self.client, days_back, limit, language, min_stars).await
    }

    /// Checks the current GitHub API rate limit status.
    ///
    /// Useful for monitoring API usage and implementing backoff strategies.
    ///
    /// # Returns
    ///
    /// Rate limit information including limit, remaining requests, and reset time.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use github_rust::GitHubService;
    /// # async fn example() -> github_rust::Result<()> {
    /// let service = GitHubService::new()?;
    /// let limits = service.check_rate_limit().await?;
    ///
    /// println!("Remaining: {}/{}", limits.remaining, limits.limit);
    /// println!("Resets at: {:?}", limits.reset_datetime());
    ///
    /// if limits.is_exceeded() {
    ///     println!("Rate limited! Wait {:?}", limits.time_until_reset());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn check_rate_limit(&self) -> Result<crate::RateLimit> {
        self.client.check_rate_limit().await
    }

    /// Returns separate REST core, search and GraphQL quotas when available.
    pub async fn check_rate_limits(&self) -> Result<crate::RateLimits> {
        self.client.check_rate_limits().await
    }

    /// Returns whether a GitHub token is configured.
    ///
    /// Useful for conditional logic based on authentication status.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use github_rust::GitHubService;
    /// let service = GitHubService::new()?;
    ///
    /// if service.has_token() {
    ///     println!("Token configured");
    /// } else {
    ///     println!("Anonymous access");
    /// }
    /// # Ok::<(), github_rust::GitHubError>(())
    /// ```
    #[must_use]
    pub fn has_token(&self) -> bool {
        self.client.has_token()
    }

    /// Gets all repositories starred by the authenticated user.
    ///
    /// Requires authentication via `GITHUB_TOKEN`.
    ///
    /// # Returns
    ///
    /// List of repository full names in "owner/repo" format.
    /// Returns a pagination error if more than 100 pages must be fetched.
    ///
    /// # Errors
    ///
    /// * [`GitHubError::AuthenticationError`] - No token or invalid token
    /// * [`GitHubError::RateLimitError`] - API rate limit exceeded
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use github_rust::GitHubService;
    /// # async fn example() -> github_rust::Result<()> {
    /// let service = GitHubService::new()?;
    /// let starred = service.get_user_starred_repositories().await?;
    ///
    /// println!("You have starred {} repositories", starred.len());
    /// for repo in starred.iter().take(5) {
    ///     println!("  - {}", repo);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_user_starred_repositories(&self) -> Result<Vec<String>> {
        rest::get_user_starred_repositories(&self.client).await
    }

    /// Gets the profile of the authenticated user.
    ///
    /// Requires authentication via `GITHUB_TOKEN`.
    ///
    /// # Errors
    ///
    /// * [`GitHubError::AuthenticationError`] - No token or invalid token
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use github_rust::GitHubService;
    /// # async fn example() -> github_rust::Result<()> {
    /// let service = GitHubService::new()?;
    /// let profile = service.get_user_profile().await?;
    ///
    /// println!("Logged in as: {}", profile.login);
    /// if let Some(name) = profile.name {
    ///     println!("Name: {}", name);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_user_profile(&self) -> Result<rest::UserProfile> {
        rest::get_user_profile(&self.client).await
    }

    /// Gets users who starred a repository with timestamps.
    ///
    /// Returns stargazers with the date they starred the repository.
    /// Supports pagination. GitHub restricts listing access to repository admins
    /// and collaborators; empty results may also reflect access restrictions.
    ///
    /// # Arguments
    ///
    /// * `owner` - Repository owner
    /// * `name` - Repository name
    /// * `per_page` - Results per page (max 100, default 30)
    /// * `page` - Page number (default 1)
    ///
    /// # Errors
    ///
    /// * [`GitHubError::NotFoundError`] - Repository not found
    /// * [`GitHubError::RateLimitError`] - API rate limit exceeded
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use github_rust::GitHubService;
    /// # async fn example() -> github_rust::Result<()> {
    /// let service = GitHubService::new()?;
    ///
    /// // Get first 100 stargazers
    /// let stargazers = service.get_repository_stargazers(
    ///     "rust-lang", "rust",
    ///     Some(100),  // per_page
    ///     Some(1),    // page
    /// ).await?;
    ///
    /// for sg in stargazers {
    ///     println!("{} starred at {}", sg.user.login, sg.starred_at);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_repository_stargazers(
        &self,
        owner: &str,
        name: &str,
        per_page: Option<u32>,
        page: Option<u32>,
    ) -> Result<Vec<crate::github::types::StargazerWithDate>> {
        rest::get_repository_stargazers(&self.client, owner, name, per_page, page).await
    }
}

impl GitHubService {
    /// Configure limits for account, owned-repository and open-work-item collections.
    pub fn with_fetch_options(mut self, options: super::FetchOptions) -> Result<Self> {
        self.fetch_options = options.validate()?;
        Ok(self)
    }

    /// Fetch the authenticated account and all organizations visible to its token.
    pub async fn get_viewer(&self) -> Result<super::Viewer> {
        super::accounts::viewer(&self.client, self.fetch_options).await
    }

    /// List repositories owned by a user or organization, including visible private,
    /// forked and archived repositories. Results are sorted by full name.
    pub async fn get_owned_repositories(&self, login: &str) -> Result<super::OwnedRepositories> {
        super::accounts::repositories(
            &self.client,
            login,
            self.fetch_options,
            None::<super::pagination::NoProgress<_>>,
        )
        .await
    }

    /// List owned repositories, lending each validated page to an asynchronous callback.
    /// Use `async |page| { ...; Ok(()) }` to borrow the page across await points.
    /// Pages are provisional until this method succeeds. A callback error or dropping
    /// the returned future cancels traversal; no background tasks are spawned.
    /// GitHub does not provide a transactional snapshot across pages.
    pub async fn get_owned_repositories_with_progress<F>(
        &self,
        login: &str,
        progress: F,
    ) -> Result<super::OwnedRepositories>
    where
        F: AsyncFnMut(&super::RepositoryPage) -> Result<()>,
    {
        super::accounts::repositories(&self.client, login, self.fetch_options, Some(progress)).await
    }
}

impl GitHubService {
    /// Fetch every open work item in one repository, sorted by update time descending.
    pub async fn get_open_issues(&self, owner: &str, name: &str) -> Result<Vec<super::Issue>> {
        self.get_open_issues_for_repositories(&[super::RepositoryCoordinates::new(owner, name)?])
            .await
    }
    /// Fetch open work items in explicit repository scopes without rediscovering ownership.
    /// Archived repositories are included if supplied; applications choose their scopes.
    pub async fn get_open_issues_for_repositories(
        &self,
        repositories: &[super::RepositoryCoordinates],
    ) -> Result<Vec<super::Issue>> {
        super::work_items::collect(
            &self.client,
            repositories,
            self.fetch_options,
            None::<super::pagination::NoProgress<_>>,
        )
        .await
    }
    /// Traverse repository scopes with bounded concurrency and lend each page to an async callback.
    /// Empty scopes return immediately without authentication or a callback.
    /// Use `async |page| { ...; Ok(()) }` to borrow the page across await points.
    /// Pages are provisional until the whole call succeeds. Callback errors and dropping
    /// the future cancel traversal. No tasks are spawned. Labels and assignees must fit
    /// their embedded 100-node pages, otherwise this returns a pagination error.
    /// Totals, cursors and repository identity are checked; GitHub offers no snapshot isolation.
    pub async fn get_open_issues_with_progress<F>(
        &self,
        repositories: &[super::RepositoryCoordinates],
        progress: F,
    ) -> Result<Vec<super::Issue>>
    where
        F: AsyncFnMut(&super::WorkItemPage<super::Issue>) -> Result<()>,
    {
        super::work_items::collect(
            &self.client,
            repositories,
            self.fetch_options,
            Some(progress),
        )
        .await
    }
}

impl GitHubService {
    /// Fetch every open work item in one repository, sorted by update time descending.
    pub async fn get_open_pull_requests(
        &self,
        owner: &str,
        name: &str,
    ) -> Result<Vec<super::PullRequest>> {
        self.get_open_pull_requests_for_repositories(&[super::RepositoryCoordinates::new(
            owner, name,
        )?])
        .await
    }
    /// Fetch open work items in explicit repository scopes without rediscovering ownership.
    /// Archived repositories are included if supplied; applications choose their scopes.
    pub async fn get_open_pull_requests_for_repositories(
        &self,
        repositories: &[super::RepositoryCoordinates],
    ) -> Result<Vec<super::PullRequest>> {
        super::work_items::collect(
            &self.client,
            repositories,
            self.fetch_options,
            None::<super::pagination::NoProgress<_>>,
        )
        .await
    }
    /// Traverse repository scopes with bounded concurrency and lend each page to an async callback.
    /// Empty scopes return immediately without authentication or a callback.
    /// Use `async |page| { ...; Ok(()) }` to borrow the page across await points.
    /// Pages are provisional until the whole call succeeds. Callback errors and dropping
    /// the future cancel traversal. No tasks are spawned. Labels and assignees must fit
    /// their embedded 100-node pages, otherwise this returns a pagination error.
    /// Totals, cursors and repository identity are checked; GitHub offers no snapshot isolation.
    pub async fn get_open_pull_requests_with_progress<F>(
        &self,
        repositories: &[super::RepositoryCoordinates],
        progress: F,
    ) -> Result<Vec<super::PullRequest>>
    where
        F: AsyncFnMut(&super::WorkItemPage<super::PullRequest>) -> Result<()>,
    {
        super::work_items::collect(
            &self.client,
            repositories,
            self.fetch_options,
            Some(progress),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_github_service_creation() {
        let service = GitHubService::new();
        assert!(service.is_ok());
    }

    #[test]
    fn test_github_service_has_token_detection() {
        let service = GitHubService::new().unwrap();
        // Token detection should work without panicking
        let _has_token = service.has_token();
    }
}
