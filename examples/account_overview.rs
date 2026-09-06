//! Run with GITHUB_TOKEN set; optionally pass a user or organization login.
use github_rust::{FetchOptions, GitHubService, RepositoryCoordinates, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let service = GitHubService::new()?.with_fetch_options(FetchOptions {
        max_concurrent_repositories: 4,
        ..Default::default()
    })?;
    let login = match std::env::args().nth(1) {
        Some(login) => login,
        None => service.get_viewer().await?.account.login,
    };
    let inventory = service
        .get_owned_repositories_with_progress(&login, async |page| {
            println!(
                "Received {} of {} repositories",
                page.repositories.len(),
                page.total_count
            );
            Ok(())
        })
        .await?;
    // Application policy: exclude archives and skip repositories with no open items.
    let scopes = |issues: bool| -> Result<Vec<RepositoryCoordinates>> {
        inventory
            .repositories
            .iter()
            .filter(|r| {
                !r.is_archived
                    && if issues {
                        r.open_issue_count > 0
                    } else {
                        r.open_pull_request_count > 0
                    }
            })
            .map(|r| r.coordinates())
            .collect()
    };
    let issues = service
        .get_open_issues_with_progress(&scopes(true)?, async |page| {
            println!(
                "{}: received {} issues",
                page.repository.name_with_owner,
                page.items.len()
            );
            Ok(())
        })
        .await?;
    let pull_requests = service
        .get_open_pull_requests_for_repositories(&scopes(false)?)
        .await?;
    println!(
        "Complete: {} repositories, {} open issues, {} open PRs",
        inventory.repositories.len(),
        issues.len(),
        pull_requests.len()
    );
    Ok(())
}
