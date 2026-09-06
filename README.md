# github-rust

An async Rust library for GitHub accounts, repositories, open issues and pull requests, search and stargazer access through REST and GraphQL.

## Installation

Requires Rust 1.92 or later (edition 2024).

```toml
[dependencies]
github-rust = { git = "https://github.com/davlgd/github-rust" }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

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

These methods require a token. Empty repository batches return an empty list without authentication or callbacks.

| Method | Returns |
| --- | --- |
| `get_viewer()` | Authenticated account and its visible organizations |
| `get_owned_repositories(login)` | Repositories owned by a user or organization |
| `get_open_issues(owner, name)` | Open issues with authors, labels, assignees and comment counts |
| `get_open_pull_requests(owner, name)` | Open PRs, including draft status and review decisions |

Repository inventories include visible private repositories, forks and archives.
For batch calls, select repositories in your application and convert them with `RepositorySummary::coordinates()`.

### Progress

Use `get_open_issues_with_progress()` or `get_open_pull_requests_with_progress()` to process pages as they arrive:

```rust
async fn example(service: &github_rust::GitHubService) -> github_rust::Result<()> {
    let scopes = [github_rust::RepositoryCoordinates::new("rust-lang", "rust")?];
    let issues = service.get_open_issues_with_progress(&scopes, async |page| {
        println!("Received {} issues", page.items.len());
        Ok(())
    }).await?;
    println!("Complete: {} issues", issues.len());
    Ok(())
}
```

- Callbacks borrow pages without copying them and run serially in arrival order.
- Pages remain provisional until the call succeeds. Return an error or drop the future to cancel.
- Final issue and PR lists sort by update time descending, then node ID.
- Use `get_open_*_for_repositories()` when you only need the final list.

See [account_overview.rs](examples/account_overview.rs) for account discovery, repository filtering and batch calls.

### Send callbacks

Generic adapters using `async |page|` can encounter `Send is not general enough` when passed to `tokio::spawn` ([#3](https://github.com/davlgd/github-rust/issues/3)).

Use `async move |page|` to capture the generic callback by value, and give that callback owned state (`Arc` for shared state).
The [Send adapter example](examples/send_progress.rs) shows the bounds and a Tokio task.

A normal `move` closure returning an explicitly typed `Pin<Box<dyn Future<Output = Result<()>> + Send>>` also works for owned callback futures.

That adapter clones pages because its callback accepts owned data. Ordinary borrowed progress callbacks remain available without those copies.

### Limits

Configure collection limits with `GitHubService::with_fetch_options()`:

| `FetchOptions` field | Default |
| --- | --- |
| `page_size` | 100 nodes |
| `max_pages` | 500 per connection |
| `max_concurrent_repositories` | 4 per call |

- Changing totals, duplicate IDs, invalid cursors and repository scope changes produce errors.
- Reaching a page cap or receiving incomplete labels or assignees produces a pagination error. Embedded metadata is limited to 100 labels and 100 assignees per item.
- GitHub does not provide a snapshot across pages; concurrent changes may remain undetected.

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

Use `GitHubError::kind()` for application-facing messages. Raw `Display` output preserves upstream details and should stay in trusted diagnostics.

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
