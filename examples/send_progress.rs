//! Forward repository pages through a generic Send callback in a Tokio task.
//! Set GITHUB_TOKEN and pass an optional account login to run this example.
use github_rust::{GitHubService, RepositoryPage, Result};
use std::future::Future;

async fn collect<F, Fut>(service: &GitHubService, login: &str, mut progress: F) -> Result<()>
where
    F: FnMut(RepositoryPage) -> Fut + Send,
    Fut: Future<Output = Result<()>> + Send,
{
    service
        .get_owned_repositories_with_progress(login, async move |page| {
            // Own the callback in the async closure instead of borrowing it.
            // This adapter forwards owned pages; the library itself lends them.
            progress(page.clone()).await
        })
        .await?;
    Ok(())
}

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let service = GitHubService::new()?;
    let login = match std::env::args().nth(1) {
        Some(login) => login,
        None => service.get_viewer().await?.account.login,
    };
    tokio::spawn(async move {
        collect(&service, &login, |page| async move {
            println!(
                "{}: received {} repositories",
                page.owner.login,
                page.repositories.len()
            );
            Ok(())
        })
        .await
    })
    .await??;
    Ok(())
}
