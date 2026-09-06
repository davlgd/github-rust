use super::{
    accounts::{account, connection, repository},
    work_items::{item, response},
};
use crate::support::client;
use futures_util::{Stream, StreamExt, TryStreamExt, stream::BoxStream};
use github_rust::{ErrorKind, FetchOptions, GitHubService, RepositoryCoordinates, Result};
use serde_json::{Value, json};
use std::{future::Future, pin::pin};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_partial_json, method, path},
};

#[derive(Clone, Copy)]
enum Collection {
    Repositories,
    Issues,
    PullRequests,
}
const COLLECTIONS: [Collection; 3] = [
    Collection::Repositories,
    Collection::Issues,
    Collection::PullRequests,
];

#[test]
fn invalid_collection_limits_identify_the_field() {
    let client = github_rust::GitHubClient::builder().build().unwrap();
    for (options, field) in [
        (
            FetchOptions {
                page_size: 0,
                ..Default::default()
            },
            "page_size",
        ),
        (
            FetchOptions {
                page_size: 101,
                ..Default::default()
            },
            "page_size",
        ),
        (
            FetchOptions {
                max_pages: 0,
                ..Default::default()
            },
            "max_pages",
        ),
        (
            FetchOptions {
                max_concurrent_repositories: 0,
                ..Default::default()
            },
            "max_concurrent_repositories",
        ),
        (
            FetchOptions {
                page_size: 2,
                max_pages: usize::MAX,
                ..Default::default()
            },
            "max_pages multiplied by page_size",
        ),
    ] {
        let Err(error) = GitHubService::with_client(client.clone()).with_fetch_options(options)
        else {
            panic!("accepted invalid {field}");
        };
        assert_eq!(error.kind(), ErrorKind::InvalidInput);
        assert!(error.to_string().contains(field));
    }
}

fn scopes() -> [RepositoryCoordinates; 1] {
    [RepositoryCoordinates::new("owner", "repo").unwrap()]
}

impl Collection {
    fn body(self, index: usize, total: usize, next: Option<&str>) -> Value {
        match self {
            Self::Repositories => {
                let mut owner = account();
                owner["repositories"] = connection(vec![repository(index)], total, next);
                json!({"data": {"repositoryOwner": owner}})
            }
            Self::Issues | Self::PullRequests => {
                let issues = matches!(self, Self::Issues);
                let mut node = item(index);
                if !issues {
                    node["isDraft"] = json!(false);
                }
                response("repo", vec![node], total, next, issues)
            }
        }
    }

    // Run the same HTTP scenarios against each collection type.
    fn pages(self, service: &GitHubService) -> BoxStream<'static, Result<usize>> {
        match self {
            Self::Repositories => service
                .get_owned_repository_pages("owner")
                .map_ok(|page| page.repositories.len())
                .boxed(),
            Self::Issues => service
                .get_open_issue_pages(&scopes())
                .map_ok(|page| page.items.len())
                .boxed(),
            Self::PullRequests => service
                .get_open_pull_request_pages(&scopes())
                .map_ok(|page| page.items.len())
                .boxed(),
        }
    }
}

async fn mount(server: &MockServer, cursor: Option<&str>, body: Value, expected: u64) {
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(body_partial_json(json!({"variables": {"cursor": cursor}})))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(expected)
        .mount(server)
        .await;
}

async fn forward<P, S, F, Fut>(pages: S, mut progress: F) -> Result<()>
where
    P: Send,
    S: Stream<Item = Result<P>> + Send,
    F: FnMut(P) -> Fut + Send,
    Fut: Future<Output = Result<()>> + Send,
{
    let mut pages = pin!(pages);
    while let Some(page) = pages.try_next().await? {
        progress(page).await?;
    }
    Ok(())
}

async fn exercise<P, S>(pages: S, ids: fn(P) -> Vec<String>) -> Vec<String>
where
    P: Send,
    S: Stream<Item = Result<P>> + Send,
{
    let mut received = vec![];
    let label = String::from("borrowed across await");
    let borrowed_label = &label;
    let operation = forward(pages, |page| {
        received.extend(ids(page));
        async move {
            tokio::task::yield_now().await;
            assert_eq!(borrowed_label, "borrowed across await");
            Ok(())
        }
    });
    fn require_send<T: Future + Send>(future: T) -> T {
        future
    }
    require_send(operation).await.unwrap();
    received
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn generic_consumers_can_borrow_local_state_in_tokio_tasks() {
    for collection in COLLECTIONS {
        let server = MockServer::start().await;
        mount(&server, None, collection.body(0, 2, Some("next")), 1).await;
        mount(&server, Some("next"), collection.body(1, 2, None), 1).await;
        let service = GitHubService::with_client(client(&server, true));
        // Streams outlive their service and temporary scope arguments.
        let received = match collection {
            Collection::Repositories => {
                let pages = service.get_owned_repository_pages(&String::from("owner"));
                drop(service);
                tokio::spawn(exercise(pages, |page| {
                    page.repositories.into_iter().map(|r| r.node_id).collect()
                }))
                .await
                .unwrap()
            }
            Collection::Issues => {
                let pages = service.get_open_issue_pages(&scopes());
                drop(service);
                tokio::spawn(exercise(pages, |page| {
                    page.items.into_iter().map(|i| i.node_id).collect()
                }))
                .await
                .unwrap()
            }
            Collection::PullRequests => {
                let pages = service.get_open_pull_request_pages(&scopes());
                drop(service);
                tokio::spawn(exercise(pages, |page| {
                    page.items.into_iter().map(|pr| pr.node_id).collect()
                }))
                .await
                .unwrap()
            }
        };
        let expected = if matches!(collection, Collection::Repositories) {
            ["R_0", "R_1"]
        } else {
            ["I_0", "I_1"]
        };
        assert_eq!(received, expected);
    }
}

#[tokio::test]
async fn streams_are_lazy_and_stopping_after_a_page_cancels_traversal() {
    for collection in COLLECTIONS {
        let server = MockServer::start().await;
        mount(&server, None, collection.body(0, 2, Some("next")), 1).await;
        mount(&server, Some("next"), collection.body(1, 2, None), 0).await;
        let service = GitHubService::with_client(client(&server, true));
        let mut pages = collection.pages(&service);
        assert!(server.received_requests().await.unwrap().is_empty());
        assert_eq!(pages.try_next().await.unwrap(), Some(1));
        tokio::task::yield_now().await;
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
        drop(pages);
        tokio::task::yield_now().await;
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }
}

#[tokio::test]
async fn late_pagination_errors_are_reported_once_and_end_the_stream() {
    for collection in COLLECTIONS {
        let server = MockServer::start().await;
        mount(&server, None, collection.body(0, 2, Some("next")), 1).await;
        // The total changed since the first page: never yield this invalid page.
        mount(&server, Some("next"), collection.body(1, 3, None), 1).await;
        let service = GitHubService::with_client(client(&server, true));
        let mut pages = collection.pages(&service);
        assert_eq!(pages.try_next().await.unwrap(), Some(1));
        assert_eq!(
            pages.try_next().await.unwrap_err().kind(),
            ErrorKind::Pagination
        );
        assert!(pages.next().await.is_none());
        assert!(pages.next().await.is_none());
    }
}

#[tokio::test]
async fn empty_scopes_and_validation_errors_do_not_send_requests() {
    let server = MockServer::start().await;
    let service = GitHubService::with_client(client(&server, false));
    assert!(
        pin!(service.get_open_issue_pages(&[]))
            .next()
            .await
            .is_none()
    );
    assert!(
        pin!(service.get_open_pull_request_pages(&[]))
            .next()
            .await
            .is_none()
    );
    let mut invalid_owner = pin!(service.get_owned_repository_pages("../bad"));
    assert_eq!(
        invalid_owner.try_next().await.unwrap_err().kind(),
        ErrorKind::InvalidInput
    );
    assert!(invalid_owner.next().await.is_none());
    let duplicate = [scopes()[0].clone(), scopes()[0].clone()];
    let mut invalid_scopes = pin!(service.get_open_issue_pages(&duplicate));
    assert_eq!(
        invalid_scopes.try_next().await.unwrap_err().kind(),
        ErrorKind::InvalidInput
    );
    assert!(invalid_scopes.next().await.is_none());
    for collection in COLLECTIONS {
        let mut pages = collection.pages(&service);
        assert_eq!(
            pages.try_next().await.unwrap_err().kind(),
            ErrorKind::Authentication
        );
        assert!(pages.next().await.is_none());
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn empty_inventory_yields_its_owner_and_then_ends() {
    let server = MockServer::start().await;
    let mut owner = account();
    owner["repositories"] = connection(vec![], 0, None);
    mount(
        &server,
        None,
        json!({"data": {"repositoryOwner": owner}}),
        1,
    )
    .await;
    let service = GitHubService::with_client(client(&server, true));
    let mut pages = pin!(service.get_owned_repository_pages("owner"));
    let page = pages.try_next().await.unwrap().unwrap();
    assert_eq!(page.owner.node_id, "U_owner");
    assert!(page.repositories.is_empty());
    assert!(!page.has_next_page);
    assert!(pages.next().await.is_none());
    assert!(pages.next().await.is_none());
}

#[tokio::test]
async fn dropping_work_item_streams_does_not_start_queued_scopes() {
    for issues in [true, false] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"variables": {"name": "repo"}})))
            .respond_with(ResponseTemplate::new(200).set_body_json(response(
                "repo",
                vec![],
                0,
                None,
                issues,
            )))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"variables": {"name": "queued"}})))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        let service = GitHubService::with_client(client(&server, true))
            .with_fetch_options(FetchOptions {
                max_concurrent_repositories: 1,
                ..Default::default()
            })
            .unwrap();
        let scopes = [
            scopes()[0].clone(),
            RepositoryCoordinates::new("owner", "queued").unwrap(),
        ];
        if issues {
            let mut pages = Box::pin(service.get_open_issue_pages(&scopes));
            assert!(pages.try_next().await.unwrap().unwrap().items.is_empty());
            drop(pages);
        } else {
            let mut pages = Box::pin(service.get_open_pull_request_pages(&scopes));
            assert!(pages.try_next().await.unwrap().unwrap().items.is_empty());
            drop(pages);
        }
        tokio::task::yield_now().await;
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }
}

#[tokio::test]
async fn borrowed_callbacks_still_support_non_send_state() {
    use std::{cell::Cell, rc::Rc};
    let server = MockServer::start().await;
    mount(&server, None, Collection::Repositories.body(0, 1, None), 1).await;
    let service = GitHubService::with_client(client(&server, true));
    let received = Rc::new(Cell::new(0));
    let result = service
        .get_owned_repositories_with_progress("owner", async |page| {
            tokio::task::yield_now().await;
            received.set(received.get() + page.repositories.len());
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(received.get(), result.repositories.len());
}
