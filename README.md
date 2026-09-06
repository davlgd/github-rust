# github-rust

An async Rust library for GitHub accounts, repositories, open issues and pull requests, search and stargazer access through REST and GraphQL.

This checkout contains the v0.2.0 API. See [CHANGELOG.md](CHANGELOG.md) for changes from v0.1.0.

## Using this checkout

```toml
[dependencies]
github-rust = { path = "../github-rust" }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

Rust 1.92 or later is required; the library uses edition 2024. The path dependency above works before registry publication of v0.2.0.

## Quick start

```rust
use github_rust::{GitHubService, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let service = GitHubService::new()?;
    let repo = service.get_repository_info("rust-lang", "rust").await?;
    println!("{}: {} stars", repo.name_with_owner, repo.stargazer_count);
    println!("Node ID: {}", repo.node_id);
    if let Some(id) = repo.database_id {
        println!("Database ID: {id}");
    }
    if let Some(count) = repo.open_issues() {
        println!("Open issues: {count}");
    }
    Ok(())
}
```

Without a token, repository lookups use REST directly. With a token, they use GraphQL and fall back to REST only on HTTP 502, 503 or 504. Authentication, authorization, quota and decoding errors are returned without fallback. A failed fallback retains both causes in `GitHubError::FallbackError`.

## Configuration and authentication

`GitHubService::new()` reads `GITHUB_TOKEN` from the environment:

```bash
export GITHUB_TOKEN="your-token"
```

The library does **not** load `.env` files. Load them in your application if needed.

For explicit configuration, use the builder. It does not read the environment and is anonymous unless a token is supplied:

```rust
use github_rust::{FallbackPolicy, GitHubClient, GitHubService};

fn main() -> github_rust::Result<()> {
let client = GitHubClient::builder()
    .token("your-token".into())
    .build()?;
let service = GitHubService::with_client(client)
    .with_fallback_policy(FallbackPolicy::Never);
Ok(())
}
```

The builder also accepts `rest_url`, `graphql_url`, and an injected `reqwest::Client` through `http_client`. This supports local mocks and GitHub Enterprise endpoints. Configure both URLs for a custom installation; API availability depends on the server version and permissions. Custom endpoints receive the token, so use trusted URLs. An injected transport controls its own timeout, proxy and redirect policy; the default transport has a 30-second request timeout.

The retained token uses `secrecy::SecretString` and is zeroized when dropped. HTTP requests also contain copies of its value in authorization headers; those copies are not covered by this zeroization guarantee. `GitHubClient::client()` exposes the raw transport without the library's authentication headers.

## Repository data

Repository models use ordinary Rust fields and serialize with snake_case keys, independently of the transport's JSON format:

- `node_id` is an opaque string. `database_id` is an optional numeric ID. Do not decode node IDs.
- `watcher_count` counts notification subscribers, independently of `stargazer_count`.
- `open_issue_count` excludes pull requests. It is unavailable from the REST repository response.
- `pull_request_count` includes all states, and `release_count` counts releases. Both are unavailable from the REST repository response.
- An unavailable count is `None`; an observed zero is `Some(0)`.
- `topics` is a `Vec<String>`; `default_branch` is an `Option<String>`.
- `languages` is an optional list of `LanguageUsage { language, bytes }`. A REST 404 for the language breakdown produces `None`. Other failures are returned. GraphQL retrieves up to 100 languages; check `languages_complete` before treating the list as exhaustive.

Helpers include `language()`, `license()`, `license_spdx()`, `topics()`, `owner()`, `default_branch()`, `open_issues()` and `watcher_count()`.

## Accounts, issues and pull requests

Authenticated collection methods support complete pagination and asynchronous progress callbacks:

```rust
async fn example(service: &github_rust::GitHubService) -> github_rust::Result<()> {
    let viewer = service.get_viewer().await?;
    let inventory = service.get_owned_repositories(&viewer.account.login).await?;
    let scopes = inventory.repositories.iter()
        .filter(|r| !r.is_archived && r.open_issue_count > 0)
        .map(|r| r.coordinates()).collect::<github_rust::Result<Vec<_>>>()?;
    let issues = service.get_open_issues_with_progress(&scopes, async |page| {
        println!("{}: {} issues received", page.repository.name_with_owner, page.items.len());
        Ok(())
    }).await?;
    println!("Complete: {} issues", issues.len());
    Ok(())
}
```

The matching `get_open_pull_requests*` methods return PR-specific draft and review fields.
Use `with_fetch_options(FetchOptions { ..Default::default() })` to configure pagination and concurrent repository requests.
Async callbacks borrow provisional pages without cloning them; only successful completion certifies the full traversal. Returning an error or dropping the future stops traversal.
The inventory includes visible private repositories, forks and archives owned by the requested account.
Empty batches succeed without authentication or callbacks. Other collection calls require a token.
Batch methods accept explicit scopes without rediscovering ownership; applications choose whether to include archives or skip zero counts.
Final work-item lists sort by update time descending, then node ID; callbacks across repositories arrive serially in completion order.

Collection defaults are 100 nodes per page, 500 pages per connection and four concurrent repositories per call.
Changing totals, duplicate node IDs, cursor cycles, empty progress, scope changes and exhausted caps produce errors.
Labels and assignees must fit their embedded pages of 100; larger reported totals cause a pagination error instead of truncated success.
These checks cannot provide snapshot isolation: GitHub data may change between requests without a detectable count change.

Use `GitHubError::kind()` for application-owned error messages; raw `Display` output preserves upstream details and should not be exposed to untrusted clients.
Credential resolution, cache policy and request-level scheduling belong to the application; concurrency limits apply independently to each call.
See the [account overview example](examples/account_overview.rs) for inventory reuse and filtering.

## Search

Repository search requires a token and uses GraphQL:

```rust
async fn example(service: &github_rust::GitHubService) -> github_rust::Result<()> {
let repos = service.search_repositories(30, 100, Some("Rust"), 50).await?;
for repo in repos {
    println!("{}: {} stars", repo.name_with_owner, repo.stargazer_count);
}
Ok(())
}
```

Search selects public repositories created after the date `days_back` days ago, with at least `min_stars`, ordered by stars descending. It caps results at 1,000 and requests only the number still needed. A zero limit returns an empty list without a request. Invalid dates or language syntax produce `InvalidInput`; missing, repeated or nonprogressing pagination cursors produce `PaginationError`.

## Stargazers and starred repositories

```rust
async fn example(service: &github_rust::GitHubService) -> github_rust::Result<()> {
let stargazers = service.get_repository_stargazers("owner", "repo", Some(100), Some(1)).await?;
for star in stargazers {
    println!("{} starred at {}", star.user.login, star.starred_at);
}
Ok(())
}
```

GitHub [announced restrictions on stargazer listings](https://github.blog/changelog/2026-06-30-upcoming-access-restrictions-to-public-api-endpoints-and-ui-views/) to administrators and collaborators. A public repository alone does not guarantee access: GitHub documents possible empty responses or HTTP 403. This method reflects the server's response and cannot distinguish an access-filtered empty list from a repository with no stars. It is not a general public star-history API.

`per_page` defaults to 30 and is capped at 100; `page` defaults to 1. Zero values are rejected. The first page is not guaranteed to contain the most recent stars.

`get_user_profile()` and `get_user_starred_repositories()` require an authorized token. The starred-repository helper returns repository names and stops after 100 pages. If another page is needed at that point, it returns `PaginationError` instead of returning a silently truncated list.

## Quotas and errors

```rust
async fn example(service: &github_rust::GitHubService) -> github_rust::Result<()> {
let quotas = service.check_rate_limits().await?;
for (resource, limit) in quotas.resources {
    println!("{resource}: {}/{} remaining", limit.remaining, limit.limit);
    println!("Reset: {:?}", limit.reset_datetime());
}
Ok(())
}
```

`check_rate_limit()` is a convenience method for the REST `core` quota. Search and GraphQL have separate quotas; token presence alone does not establish the available allowance. `RateLimit` provides `used()`, `is_exceeded()`, `time_until_reset()` and `reset_datetime()`; an unrepresentable timestamp returns `None` from the last helper.

`RateLimitError` contains `RateLimitDetails`: the HTTP status, resource, remaining allowance, reset timestamp, original `Retry-After`, request ID and GraphQL errors when present. The library does not automatically sleep or retry. Applications can use this metadata to implement a bounded retry policy.

Other errors distinguish authentication, permission denial, unavailable resources, legal restrictions, API status errors, typed GraphQL errors, pagination and response parsing. Transport and decoding errors preserve their original `std::error::Error::source()` chains. GraphQL errors are returned even if the response also includes partial data.

## Development

```bash
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked
cargo test --doc --locked
cargo doc --no-deps --locked
```

Tests use local HTTP mocks and do not require a token or contact GitHub. The single live API test is ignored by default; run it explicitly with `cargo test --test github_api_tests test_real_github_api_rate_limit -- --ignored`.

CI checks Rust 1.92 and stable on Linux, plus stable on macOS. Examples:

```bash
cargo run --example basic_usage
cargo run --example search_repositories
cargo run --example stargazers -- owner/repo
```

## License

Apache-2.0.
