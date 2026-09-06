# GitHub Rust Library

Small async Rust library for GitHub accounts, repositories, issues, PRs, search and stargazers.
Record user-visible changes in CHANGELOG.md;
version metadata does not itself publish a tag, GitHub release or registry package.

## Layout

- `src/github/client.rs`: explicit builder, HTTP transport, authentication and quotas.
- `src/github/accounts.rs`, `work_items.rs`, `pagination.rs`: account and work-item collections.
- `src/github/queries/`: embedded GraphQL collection queries.
- `src/github/models.rs`: public repository models independent of transport JSON.
- `src/github/graphql.rs`, `rest.rs`, `search.rs`: wire DTOs and API operations.
- `src/github/response.rs`: shared HTTP and GraphQL error classification.
- `src/github/service.rs`: public entry point and conservative fallback policy.
- `src/error.rs`: typed errors with original causes and retry metadata.
- `tests/cases/`, `tests/support/`: HTTP mock scenarios and fixtures, included by `tests/github_api_tests.rs`.

## Contracts

- Node IDs are opaque. Use official numeric database IDs instead of decoding.
- Unknown counts are `None`, never fabricated zeros.
- GraphQL issue counts filter OPEN; REST does not provide the equivalent count.
- Anonymous repository lookup goes directly to REST. Authenticated fallback only
  handles HTTP 502, 503 and 504, and may be disabled.
- Search requires a token. Pagination must make progress and respect result caps.
- Builder configuration is explicit; only `GitHubClient::new()` reads GITHUB_TOKEN.
- Never mutate global environment variables in concurrent tests.
- API methods attach authentication to requests; raw transport access does not.
- Public API changes are documented in README.md and CHANGELOG.md.

## Validation

Rust 2024 edition, MSRV 1.92. Default tests use local HTTP mocks without GitHub access.

```bash
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked
cargo test --doc --locked
cargo doc --no-deps --locked
```

The live GitHub quota test is explicitly ignored by default.
