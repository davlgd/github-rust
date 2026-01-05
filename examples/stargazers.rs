//! Get repository stargazers with timestamps.
//!
//! Run with: `cargo run --example stargazers`

use github_rust::{GitHubService, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let service = GitHubService::new()?;

    let owner = "rust-lang";
    let repo = "rust";

    println!("Fetching recent stargazers for {}/{}...\n", owner, repo);

    // Get the first page of stargazers (most recent first is not guaranteed by GitHub API)
    let stargazers = service
        .get_repository_stargazers(owner, repo, Some(10), Some(1))
        .await?;

    println!("Showing {} stargazers:\n", stargazers.len());

    for sg in &stargazers {
        println!("  {} starred at {}", sg.user.login, sg.starred_at);
        println!("    Profile: {}", sg.user.html_url);
    }

    // Demonstrate pagination
    println!("\n--- Fetching page 2 ---\n");

    let page2 = service
        .get_repository_stargazers(owner, repo, Some(5), Some(2))
        .await?;

    for sg in &page2 {
        println!("  {} starred at {}", sg.user.login, sg.starred_at);
    }

    Ok(())
}
