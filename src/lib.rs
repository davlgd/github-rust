//! # GitHub Rust - Core Library
//!
//! A Rust library for GitHub API integration with dual GraphQL/REST support.
//!
//! ## Features
//!
//! - **Dual API**: GraphQL primary with REST fallback
//! - **Search**: Repository search with filters
//! - **Performance**: Connection pooling and async I/O
//! - **Type Safety**: Comprehensive error handling
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use github_rust::GitHubService;
//!
//! # async fn example() -> github_rust::Result<()> {
//! let service = GitHubService::new()?;
//!
//! // Get repository info
//! let repo = service.get_repository_info("microsoft", "vscode").await?;
//! println!("{}: {} stars", repo.name_with_owner, repo.stargazer_count);
//!
//! // Search recent repositories (30 days back, limit 10, Rust language, min 100 stars)
//! let repos = service.search_repositories(30, 10, Some("rust"), 100).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Authentication
//!
//! Set `GITHUB_TOKEN` environment variable for higher rate limits (5000/hour vs 60/hour).

pub mod config;
pub mod error;
pub mod github;

pub use config::{GITHUB_API_URL, GITHUB_GRAPHQL_URL};
pub use error::{DecodeError, GitHubError, RateLimitDetails, Result};
pub use github::{
    FallbackPolicy, GitHubClient, GitHubClientBuilder, GitHubService, LanguageUsage, RateLimit,
    RateLimits, Repository, SearchRepository, StargazerWithDate, User, UserProfile,
};

use base64::Engine;

/// Best-effort decoder for historical node ID formats. Returns 0 on failure.
///
/// Node IDs are opaque: new formats are not guaranteed to be supported.
/// Use the `node_id` and `database_id` fields returned by repository operations.
#[deprecated(
    since = "0.2.0",
    note = "GitHub node IDs are opaque; use Repository::database_id or SearchRepository::database_id"
)]
pub fn parse_github_node_id(node_id: &str) -> i64 {
    // New format: R_kgDO... (URL-safe base64-encoded msgpack)
    if let Some(b64_part) = node_id.strip_prefix("R_") {
        // Decode URL-safe base64 (add padding if needed)
        let padded = match b64_part.len() % 4 {
            2 => format!("{}==", b64_part),
            3 => format!("{}=", b64_part),
            _ => b64_part.to_string(),
        };

        if let Ok(bytes) = base64::engine::general_purpose::URL_SAFE.decode(&padded) {
            // msgpack format: 0x92 (array of 2), 0x00 (type), 0xce (uint32), 4 bytes ID
            if bytes.len() >= 7 && bytes[0] == 0x92 && bytes[2] == 0xce {
                let id_bytes = &bytes[3..7];
                return i64::from(u32::from_be_bytes([
                    id_bytes[0],
                    id_bytes[1],
                    id_bytes[2],
                    id_bytes[3],
                ]));
            }
        }
    }

    // Legacy format: MDEwOlJlcG9zaXRvcnk... (standard base64 "010:Repository{id}")
    if node_id.starts_with("MDEw")
        && let Some(id) = base64::engine::general_purpose::STANDARD
            .decode(node_id)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .and_then(|s| {
                s.strip_prefix("010:Repository")
                    .and_then(|n| n.parse::<i64>().ok())
            })
    {
        return id;
    }

    // Fallback: return 0 for invalid IDs
    tracing::warn!("Failed to parse GitHub node ID: {}", node_id);
    0
}

/// Parse a repository string "owner/repo" into components.
///
/// # Examples
///
/// ```
/// use github_rust::parse_repository;
///
/// let (owner, repo) = parse_repository("microsoft/vscode").unwrap();
/// assert_eq!(owner, "microsoft");
/// assert_eq!(repo, "vscode");
///
/// assert!(parse_repository("invalid").is_err());
/// ```
pub fn parse_repository(repo: &str) -> std::result::Result<(String, String), String> {
    let parts: Vec<&str> = repo.split('/').collect();
    if parts.len() != 2 {
        return Err(format!(
            "Invalid repository format '{}'. Use 'owner/repo'",
            repo
        ));
    }

    let owner = parts[0].trim();
    let name = parts[1].trim();

    if owner.is_empty() || name.is_empty() {
        return Err(format!(
            "Invalid repository format '{}'. Owner and repository name cannot be empty",
            repo
        ));
    }

    Ok((owner.to_string(), name.to_string()))
}

#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_repository_valid() {
        let (owner, repo) = parse_repository("owner/repo").unwrap();
        assert_eq!(owner, "owner");
        assert_eq!(repo, "repo");
    }

    #[test]
    fn test_parse_repository_with_whitespace() {
        let (owner, repo) = parse_repository(" owner / repo ").unwrap();
        assert_eq!(owner, "owner");
        assert_eq!(repo, "repo");
    }

    #[test]
    fn test_parse_repository_invalid() {
        assert!(parse_repository("invalid").is_err());
        assert!(parse_repository("/repo").is_err());
        assert!(parse_repository("owner/").is_err());
        assert!(parse_repository("owner/repo/extra").is_err());
    }

    #[test]
    fn test_parse_github_node_id_new_format() {
        // R_kgDOQBnJRQ = karpathy/nanochat (ID: 1075431749)
        assert_eq!(parse_github_node_id("R_kgDOQBnJRQ"), 1075431749);
        // R_kgDOQpc_vg = productdevbook/port-killer (ID: 1117208510)
        assert_eq!(parse_github_node_id("R_kgDOQpc_vg"), 1117208510);
        // R_kgDOPQG3Mw = anomalyco/opentui (ID: 1023522611)
        assert_eq!(parse_github_node_id("R_kgDOPQG3Mw"), 1023522611);
    }

    #[test]
    fn test_parse_github_node_id_legacy_format() {
        assert_eq!(
            parse_github_node_id("MDEwOlJlcG9zaXRvcnkxMjk2MjY5"),
            1296269
        );
    }

    #[test]
    fn test_parse_github_node_id_invalid() {
        assert_eq!(parse_github_node_id("invalid"), 0);
        assert_eq!(parse_github_node_id(""), 0);
        assert_eq!(parse_github_node_id("R_invalid"), 0);
    }
}
