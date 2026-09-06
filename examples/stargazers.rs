//! Get repository stargazers with timestamps.
//!
//! Run with: `cargo run --example stargazers -- owner/repo`

use github_rust::{GitHubError, GitHubService, Result, parse_repository};

#[tokio::main]
async fn main() -> Result<()> {
    let service = GitHubService::new()?;

    let input = std::env::args().nth(1).ok_or_else(|| {
        GitHubError::InvalidInput(
            "Usage: stargazers owner/repo (requires repository access)".into(),
        )
    })?;
    let (owner, repo) = parse_repository(&input).map_err(GitHubError::InvalidInput)?;

    println!("Fetching stargazers for {}/{}...\n", owner, repo);

    // Get the first page of stargazers (most recent first is not guaranteed by GitHub API)
    let stargazers = service
        .get_repository_stargazers(&owner, &repo, Some(5), Some(1))
        .await?;

    println!("Showing {} stargazers:\n", stargazers.len());

    for sg in &stargazers {
        println!("  {} starred at {}", sg.user.login, sg.starred_at);
        println!("    Profile: {}", sg.user.html_url);
    }

    // Demonstrate pagination
    println!("\n--- Fetching page 2 ---\n");

    let page2 = service
        .get_repository_stargazers(&owner, &repo, Some(5), Some(2))
        .await?;

    for sg in &page2 {
        println!("  {} starred at {}", sg.user.login, sg.starred_at);
    }

    Ok(())
}
