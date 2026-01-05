# github-rust

A lightweight Rust library for GitHub API interactions with dual GraphQL/REST support.

## Features

- **Dual API Support**: GraphQL primary with automatic REST fallback
- **Repository Search**: Find repositories by date, stars, and language
- **Stargazer Tracking**: Get repository stargazers with timestamps
- **Rate Limiting**: Built-in rate limit checking with helper methods
- **Error Handling**: Comprehensive error types with actionable messages
- **Security**: Token stored securely with automatic zeroization on drop

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
github-rust = "0.1"
```

This library uses async/await. You'll need an async runtime to execute the code. Examples use [Tokio](https://tokio.rs/):

```toml
[dependencies]
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

## Quick Start

```rust
use github_rust::{GitHubService, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let service = GitHubService::new()?;

    // Get repository information
    let repo = service.get_repository_info("microsoft", "vscode").await?;
    println!("{}: {} stars", repo.name_with_owner, repo.stargazer_count);

    // Use helper methods
    if let Some(lang) = repo.language() {
        println!("Language: {}", lang);
    }
    println!("Topics: {:?}", repo.topics());

    Ok(())
}
```

## API Overview

### GitHubService

The main entry point for all GitHub operations.

```rust
use github_rust::GitHubService;

let service = GitHubService::new()?;

// Check if authenticated
if service.has_token() {
    println!("Using authenticated API (5000 requests/hour)");
} else {
    println!("Using unauthenticated API (60 requests/hour)");
}
```

### Repository Information

```rust
// Get full repository details (uses GraphQL, falls back to REST)
let repo = service.get_repository_info("owner", "repo").await?;

println!("Name: {}", repo.name_with_owner);
println!("Stars: {}", repo.stargazer_count);
println!("Forks: {}", repo.fork_count);
println!("Language: {:?}", repo.primary_language);
println!("License: {:?}", repo.license_info);
```

### Search Repositories

```rust
// Search for recent repositories
// Parameters: days_back, limit, language (optional), min_stars
let repos = service.search_repositories(
    30,           // Created in last 30 days
    100,          // Limit to 100 results
    Some("rust"), // Filter by language
    50,           // Minimum 50 stars
).await?;

for repo in repos {
    println!("{}: {} stars", repo.name_with_owner, repo.stargazer_count);
}
```

### Stargazers

```rust
// Get stargazers with timestamps
let stargazers = service.get_repository_stargazers(
    "microsoft", "vscode",
    Some(100),  // per_page
    Some(1),    // page
).await?;

for stargazer in stargazers {
    println!("{} starred at {}", stargazer.user.login, stargazer.starred_at);
}
```

### Rate Limits

```rust
let limits = service.check_rate_limit().await?;
println!("Remaining: {}/{}", limits.remaining, limits.limit);
println!("Used: {}", limits.used());
println!("Resets at: {}", limits.reset_datetime());

if limits.is_exceeded() {
    println!("Rate limited! Wait {:?}", limits.time_until_reset());
}
```

### User Profile

```rust
// Requires authentication
let profile = service.get_user_profile().await?;
println!("Logged in as: {}", profile.login);

// Get user's starred repositories
let starred = service.get_user_starred_repositories().await?;
println!("You have starred {} repositories", starred.len());
```

## Error Handling

The library provides comprehensive error types:

```rust
use github_rust::{GitHubError, Result};

match service.get_repository_info("owner", "repo").await {
    Ok(repo) => println!("Found: {}", repo.name),
    Err(GitHubError::NotFoundError(name)) => println!("Repository {} not found", name),
    Err(GitHubError::RateLimitError(msg)) => println!("Rate limited: {}", msg),
    Err(GitHubError::AuthenticationError(msg)) => println!("Auth error: {}", msg),
    Err(e) => println!("Other error: {}", e),
}
```

## Types

### SearchRepository

Returned by `search_repositories()`:

```rust
pub struct SearchRepository {
    pub id: String,
    pub name: String,
    pub name_with_owner: String,
    pub description: Option<String>,
    pub url: String,
    pub stargazer_count: u32,
    pub fork_count: u32,
    pub created_at: String,
    pub updated_at: String,
    pub pushed_at: Option<String>,
    pub primary_language: Option<Language>,
    pub license_info: Option<License>,
    pub repository_topics: TopicConnection,
}
```

### RateLimit

```rust
pub struct RateLimit {
    pub limit: u64,
    pub remaining: u64,
    pub reset: u64,  // Unix timestamp
}

impl RateLimit {
    pub fn reset_datetime(&self) -> DateTime<Utc>;  // When limit resets
    pub fn time_until_reset(&self) -> Duration;     // Time until reset
    pub fn is_exceeded(&self) -> bool;              // True if no requests left
    pub fn used(&self) -> u64;                      // Requests used
}
```

## Authentication

Set `GITHUB_TOKEN` environment variable to get access to private repositories and higher rate limits:

```bash
export GITHUB_TOKEN="ghp_xxxxxxxxxxxx"
```

Or use a `.env` file in your project root.

- [GitHub Tokens Settings](https://github.com/settings/tokens)

## License

Apache-2.0
