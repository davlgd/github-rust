//! Basic usage example for github-rust.
//!
//! Run with: `cargo run --example basic_usage`

use github_rust::{GitHubService, Result};

#[tokio::main]
async fn main() -> Result<()> {
    // Create a new GitHub service
    // Automatically detects GITHUB_TOKEN from environment
    let service = GitHubService::new()?;

    // Check authentication status
    if service.has_token() {
        println!("Token configured; quotas depend on the API resource");
    } else {
        println!("Anonymous access");
        println!("Set GITHUB_TOKEN for higher limits\n");
    }

    // Check current rate limit
    let limits = service.check_rate_limit().await?;
    println!(
        "Rate limit: {}/{} remaining (resets in {:?})\n",
        limits.remaining,
        limits.limit,
        limits.time_until_reset()
    );

    // Get repository information
    let repo = service.get_repository_info("rust-lang", "rust").await?;

    println!("Repository: {}", repo.name_with_owner);
    println!("  Stars: {}", repo.stargazer_count);
    println!("  Forks: {}", repo.fork_count);
    if let Some(count) = repo.open_issues() {
        println!("  Open Issues: {}", count);
    }

    if let Some(lang) = repo.language() {
        println!("  Language: {}", lang);
    }

    if let Some(license) = repo.license() {
        println!("  License: {}", license);
    }

    if let Some(branch) = repo.default_branch() {
        println!("  Default Branch: {}", branch);
    }

    let topics = repo.topics();
    if !topics.is_empty() {
        println!("  Topics: {}", topics.join(", "));
    }

    if let Some(desc) = &repo.description {
        println!("  Description: {}", desc);
    }

    Ok(())
}
