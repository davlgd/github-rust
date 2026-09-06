# Preparing for v0.2.0

These changes describe the unreleased development API. No v0.2.0 release or tag has been created, and `Cargo.toml` still declares version `0.1.0`.

## Repository models

| v0.1.0 | Development API |
| --- | --- |
| `Repository.id`, `SearchRepository.id` | `node_id: String` and `database_id: Option<u64>` |
| `watchers.total_count` | `watcher_count: Option<u32>`; REST uses subscribers, not stars |
| `issues.total_count` | `open_issue_count: Option<u32>`; GraphQL filters open issues; REST returns `None` |
| `pull_requests.total_count` | `pull_request_count: Option<u32>`; REST returns `None` |
| `releases.total_count` | `release_count: Option<u32>`; REST returns `None` |
| `repository_topics.edges` | `topics: Vec<String>` on both repository models |
| `languages.edges` | `languages: Option<Vec<LanguageUsage>>` and `languages_complete: bool` |
| `default_branch_ref` | `default_branch: Option<String>` |
| `open_issues()`, `watcher_count()` returning `u32` | Return `Option<u32>` |

Public repository JSON now uses snake_case field names. Update saved JSON, deserialization code and any downstream consumers of the previous GraphQL-shaped representation. Avoid converting unknown counts to zero unless your application explicitly wants that presentation.

`parse_github_node_id()` is deprecated. Its historical decoder has been corrected but does not promise support for future GitHub formats. Obtain numeric IDs from `database_id` instead. GitHub's [migration guidance](https://docs.github.com/en/enterprise-cloud@latest/graphql/guides/migrating-graphql-global-node-ids) requires treating node IDs as opaque strings.

## Client construction and fallback

`GitHubService::new()` still reads `GITHUB_TOKEN`. Use `GitHubClient::builder()` for explicit tokens, endpoints and injected HTTP clients. The builder does not read `GITHUB_TOKEN`; empty or malformed tokens are rejected. The library does not load `.env` files.

Use `GitHubService::with_client()` instead of constructing a service with a struct literal. Repository lookups without a token go straight to REST. Authenticated lookups fall back only for HTTP 502/503/504; use `with_fallback_policy(FallbackPolicy::Never)` to disable fallback. A failed fallback returns both errors in `GitHubError::FallbackError`.

The raw `GitHubClient::client()` accessor no longer carries library authentication headers. Use the library API methods for authenticated GitHub operations. Custom endpoints receive the token, and injected HTTP clients retain their own timeout, proxy and redirect configuration.

## Errors and quotas

- `NetworkError` carries the original `reqwest::Error`, not a string.
- `ParseError` carries `DecodeError`, preserving decoding causes when available.
- `RateLimitError` carries boxed `RateLimitDetails`, including retry and reset metadata.
- `AccessDeniedError` distinguishes permission failures from authentication and quota failures.
- `GraphQLError` preserves GitHub error types, paths and extensions.
- `PaginationError` reports missing or repeated cursors, nonprogressing pages and the starred-repository safety cap.
- `FallbackError` retains both GraphQL and REST causes.
- `RateLimit::reset_datetime()` returns `Option<DateTime<Utc>>`.
- `check_rate_limit()` reads the REST core quota; `check_rate_limits()` exposes quotas by resource name.
- The unused `MAX_RETRIES` constant has been removed. The library does not automatically retry requests.

## Pagination and access

Search requires authentication. A zero limit returns immediately; page sizes are reduced to the requested remainder. Invalid date ranges and malformed language values return `InvalidInput`. Multiword languages are quoted in the search query.

Stargazer pagination rejects `page = 0` and `per_page = 0`. Update applications that assumed every public repository's stargazers were accessible: GitHub has [announced access restrictions](https://github.blog/changelog/2026-06-30-upcoming-access-restrictions-to-public-api-endpoints-and-ui-views/), including possible empty responses and HTTP 403.

The starred-repository helper now fails at its 100-page safety cap instead of reporting partial results as a successful complete list.
