//! Regression checks for generic Tokio adapters (issue #3).
use super::{
    accounts::{account, connection, repository},
    work_items::{item, response},
};
use crate::support::client;
use github_rust::{
    GitHubService, Issue, OwnedRepositories, PullRequest, RepositoryCoordinates, RepositoryPage,
    Result, WorkItemPage,
};
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_partial_json, method, path},
};

async fn repositories<F, Fut>(service: &GitHubService, mut progress: F) -> Result<OwnedRepositories>
where
    F: FnMut(RepositoryPage) -> Fut + Send,
    Fut: Future<Output = Result<()>> + Send,
{
    service
        .get_owned_repositories_with_progress("owner", async move |page| {
            progress(page.clone()).await
        })
        .await
}
async fn issues<F, Fut>(
    service: &GitHubService,
    scopes: &[RepositoryCoordinates],
    mut progress: F,
) -> Result<Vec<Issue>>
where
    F: FnMut(WorkItemPage<Issue>) -> Fut + Send,
    Fut: Future<Output = Result<()>> + Send,
{
    service
        .get_open_issues_with_progress(scopes, async move |page| progress(page.clone()).await)
        .await
}
async fn pull_requests<F, Fut>(
    service: &GitHubService,
    scopes: &[RepositoryCoordinates],
    mut progress: F,
) -> Result<Vec<PullRequest>>
where
    F: FnMut(WorkItemPage<PullRequest>) -> Fut + Send,
    Fut: Future<Output = Result<()>> + Send,
{
    service
        .get_open_pull_requests_with_progress(scopes, async move |page| {
            progress(page.clone()).await
        })
        .await
}
async fn boxed_repositories<F, Fut>(
    service: &GitHubService,
    mut progress: F,
) -> Result<OwnedRepositories>
where
    F: FnMut(RepositoryPage) -> Fut + Send,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    service
        .get_owned_repositories_with_progress(
            "owner",
            move |page: &RepositoryPage| -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
                Box::pin(progress(page.clone()))
            },
        )
        .await
}

#[test]
fn generic_adapters_are_send_with_borrowed_service_and_scopes() {
    fn require_send<T: Future + Send>(_: T) {}
    let service = GitHubService::with_client(github_rust::GitHubClient::builder().build().unwrap());
    let scopes = [RepositoryCoordinates::new("owner", "repo").unwrap()];
    require_send(repositories(&service, |_| async { Ok(()) }));
    require_send(issues(&service, &scopes, |_| async { Ok(()) }));
    require_send(pull_requests(&service, &scopes, |_| async { Ok(()) }));
    require_send(boxed_repositories(&service, |_| async { Ok(()) }));
    // Type checking only: these futures are dropped without polling or sending requests.
}

async fn mount_pages(server: &MockServer, work_items: Option<bool>) {
    for (cursor, index, next) in [(None, 0, Some("next")), (Some("next"), 1, None)] {
        let body = if let Some(is_issue) = work_items {
            let mut node = item(index);
            if !is_issue {
                node["isDraft"] = serde_json::json!(false);
            }
            response("repo", vec![node], 2, next, is_issue)
        } else {
            let mut owner = account();
            owner["repositories"] = connection(vec![repository(index)], 2, next);
            serde_json::json!({"data": {"repositoryOwner": owner}})
        };
        let mut variables = serde_json::json!({"cursor":cursor});
        if let Some(is_issue) = work_items {
            variables["owner"] = serde_json::json!("owner");
            variables["name"] = serde_json::json!("repo");
            variables["issues"] = serde_json::json!(is_issue);
        } else {
            variables["login"] = serde_json::json!("owner");
        }
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .and(body_partial_json(
                serde_json::json!({"variables":variables}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .expect(1)
            .mount(server)
            .await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn generic_repository_adapters_run_in_tokio_tasks() {
    for boxed in [false, true] {
        let server = MockServer::start().await;
        mount_pages(&server, None).await;
        let service = GitHubService::with_client(client(&server, true));
        let received = Arc::new(Mutex::new(vec![]));
        let captured = Arc::clone(&received);
        let result = tokio::spawn(async move {
            let progress = move |page: RepositoryPage| {
                let captured = Arc::clone(&captured);
                async move {
                    tokio::task::yield_now().await;
                    captured
                        .lock()
                        .unwrap()
                        .push(page.repositories[0].node_id.clone());
                    Ok(())
                }
            };
            if boxed {
                boxed_repositories(&service, progress).await
            } else {
                repositories(&service, progress).await
            }
        })
        .await
        .unwrap()
        .unwrap();
        assert_eq!(result.repositories.len(), 2);
        assert_eq!(*received.lock().unwrap(), ["R_0", "R_1"]);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn generic_issue_adapter_runs_in_tokio_task() {
    let server = MockServer::start().await;
    mount_pages(&server, Some(true)).await;
    let service = GitHubService::with_client(client(&server, true));
    let received = Arc::new(Mutex::new(vec![]));
    let captured = Arc::clone(&received);
    let result = tokio::spawn(async move {
        let scopes = [RepositoryCoordinates::new("owner", "repo")?];
        issues(&service, &scopes, move |page| {
            let captured = Arc::clone(&captured);
            async move {
                tokio::task::yield_now().await;
                captured.lock().unwrap().push(page.items[0].node_id.clone());
                Ok(())
            }
        })
        .await
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(result.len(), 2);
    assert_eq!(*received.lock().unwrap(), ["I_0", "I_1"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn generic_pr_adapter_runs_in_tokio_task() {
    let server = MockServer::start().await;
    mount_pages(&server, Some(false)).await;
    let service = GitHubService::with_client(client(&server, true));
    let received = Arc::new(Mutex::new(vec![]));
    let captured = Arc::clone(&received);
    let result = tokio::spawn(async move {
        let scopes = [RepositoryCoordinates::new("owner", "repo")?];
        pull_requests(&service, &scopes, move |page| {
            let captured = Arc::clone(&captured);
            async move {
                tokio::task::yield_now().await;
                captured.lock().unwrap().push(page.items[0].node_id.clone());
                Ok(())
            }
        })
        .await
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(result.len(), 2);
    assert_eq!(*received.lock().unwrap(), ["I_0", "I_1"]);
}
