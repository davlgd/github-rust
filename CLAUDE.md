# CLAUDE.md - GitHub Rust Library

## Project Overview

**github-rust** is a lightweight Rust library for GitHub API integration with dual GraphQL/REST support. It provides repository search, stargazer tracking, and rate limit management with comprehensive error handling.

## Architecture

```
src/
├── lib.rs           # Public API exports, utility functions (parse_github_node_id, parse_repository)
├── config.rs        # Constants, API endpoints, GraphQL query templates
├── error.rs         # GitHubError enum and Result type alias
└── github/
    ├── mod.rs       # Module re-exports
    ├── types.rs     # Data structures (User, StargazerWithDate, Language, License, etc.)
    ├── client.rs    # GitHubClient - low-level HTTP with connection pooling, secure token storage
    ├── service.rs   # GitHubService - high-level API with GraphQL/REST fallback
    ├── graphql.rs   # GraphQL endpoint implementation + Repository type with helpers
    ├── rest.rs      # REST endpoint implementation with URL encoding
    └── search.rs    # Repository search with filtering, pagination, and input validation

examples/
├── basic_usage.rs       # Basic repository info example
├── search_repositories.rs # Search trending repos example
└── stargazers.rs        # Get stargazers with timestamps
```

## Key Design Decisions

- **GraphQL primary, REST fallback**: GraphQL is more efficient but requires auth for some operations
- **Async-first**: All API calls use async/await with Tokio runtime
- **Connection pooling**: Single `reqwest::Client` per `GitHubService` instance
- **Dual node ID parsing**: Handles both new msgpack format (`R_kgDO...`) and legacy base64
- **Secure token storage**: Uses `secrecy::SecretString` with automatic zeroization on drop
- **Input validation**: Language parameters validated, owner/name URL-encoded
- **Pagination limits**: Max 100 pages for starred repos to prevent DoS

## Commands

```bash
# Build
cargo build

# Run tests (unit tests, excludes ignored integration tests)
cargo test

# Run all tests including integration tests (requires network)
cargo test -- --include-ignored

# Check formatting
cargo fmt --check

# Lint
cargo clippy

# Build release
cargo build --release

# Generate docs
cargo doc --open
```

## Public API

Main entry point: `GitHubService`

```rust
let service = GitHubService::new()?;  // Auto-detects GITHUB_TOKEN env var

// Core methods
service.get_repository_info("owner", "repo").await?;
service.search_repositories(days_back, limit, language, min_stars).await?;
service.get_repository_stargazers("owner", "repo", per_page, page).await?;
service.check_rate_limit().await?;
service.has_token();
service.get_user_profile().await?;  // Requires auth
```

## Error Handling

`GitHubError` variants:
- `NetworkError` - Connection/timeout issues
- `AuthenticationError` - Invalid/missing token (401)
- `NotFoundError` - Repository not found (404)
- `RateLimitError` - API rate limit exceeded (403)
- `AccessBlockedError` - Repository access blocked (403)
- `DmcaBlockedError` - DMCA blocked (451)
- `InvalidInput` - Invalid input format
- `ApiError { status, message }` - Generic API errors
- `ParseError` - JSON parsing failures
- `ConfigError` - Configuration issues

## Dependencies

- **reqwest** - Async HTTP client with JSON support
- **serde/serde_json** - Serialization
- **base64** - Node ID parsing
- **percent-encoding** - URL encoding for path segments
- **chrono** - Date/time handling
- **tracing** - Structured logging
- **secrecy** - Secure token storage with zeroization

Dev: tokio, tokio-test, wiremock, tempfile

## Environment Variables

- `GITHUB_TOKEN` - GitHub personal access token (optional, enables 5000 req/hour vs 60)

## Testing Strategy

- Unit tests in each module
- Integration tests in `tests/github_api_tests.rs`
- Mock tests with `wiremock` for API responses
- Real API tests marked `#[ignore]` for optional execution

## Code Style

- Rust 2024 edition
- MSRV: 1.92
- Uses `let-else` and `if-let` chains
- Error propagation with `?` operator
- Async functions return `Result<T, GitHubError>`

## Release Profile

Optimized for size with good performance:
- `opt-level = "z"` - Size optimization
- `lto = true` - Link-time optimization
- `codegen-units = 1` - Maximum optimization
- `panic = "abort"` - Smaller binary
- `strip = true` - No debug symbols
