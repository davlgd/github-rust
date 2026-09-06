//! Search for recently created repositories with a minimum star count.
//! Requires GITHUB_TOKEN.
//!
//! Run with: `cargo run --example search_repositories`

use github_rust::{GitHubService, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let service = GitHubService::new()?;

    println!("Searching for recently created Rust repositories...\n");

    // Search for Rust repos created in the last 30 days with at least 50 stars
    let repos = service
        .search_repositories(
            30,           // days back
            10,           // limit
            Some("rust"), // language filter
            50,           // minimum stars
        )
        .await?;

    println!("Found {} repositories:\n", repos.len());

    for (i, repo) in repos.iter().enumerate() {
        println!(
            "{}. {} ({} stars)",
            i + 1,
            repo.name_with_owner,
            repo.stargazer_count
        );

        if let Some(desc) = &repo.description {
            // Truncate long descriptions (UTF-8 safe)
            let is_truncated = desc.chars().count() > 77;
            let desc: String = desc.chars().take(77).collect();
            if is_truncated {
                println!("   {}...", desc);
            } else {
                println!("   {}", desc);
            }
        }

        if let Some(lang) = repo.language() {
            println!("   Language: {}", lang);
        }

        let topics = repo.topics();
        if !topics.is_empty() {
            println!(
                "   Topics: {}",
                topics
                    .iter()
                    .take(5)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }

        println!("   URL: {}", repo.url);
        println!();
    }

    Ok(())
}
