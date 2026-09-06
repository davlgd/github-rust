# GitHub Rust Library

Small async Rust library for GitHub repository metadata, search and stargazers.
The development API prepares v0.2.0; do not bump versions, tag or publish as part
of this preparation. CHANGELOG.md records only the initial v0.1.0 public release.

## Layout

- `src/github/client.rs`: explicit builder, HTTP transport, authentication and quotas.
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
- Public API changes are documented in MIGRATING.md and README.md.

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
