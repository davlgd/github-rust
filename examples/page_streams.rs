//! Process repository pages in a Tokio task.
//! Set GITHUB_TOKEN and pass an optional account login to run this example.
use futures_util::TryStreamExt;
use github_rust::GitHubService;
use std::pin::pin;

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let service = GitHubService::new()?;
    let login = match std::env::args().nth(1) {
        Some(login) => login,
        None => service.get_viewer().await?.account.login,
    };
    let pages = service.get_owned_repository_pages(&login);
    tokio::spawn(async move {
        let mut pages = pin!(pages);
        let mut total = 0;
        while let Some(page) = pages.try_next().await? {
            total += page.repositories.len();
            println!("{}: received {} repositories", page.owner.login, total);
        }
        println!("Complete: {total} repositories");
        github_rust::Result::Ok(())
    })
    .await??;
    Ok(())
}
