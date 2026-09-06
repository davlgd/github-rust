use super::accounts::connection;
use crate::support::client;
use github_rust::{
    ErrorKind, FetchOptions, GitHubError, GitHubService, RepositoryCoordinates, ReviewDecision,
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_partial_json, method, path},
};

pub(super) fn item(id: usize) -> Value {
    json!({"id":format!("I_{id}"),"number":id+1,"title":"A work item","url":"https://github.com/owner/repo/issues/1",
        "createdAt":"2026-01-01T00:00:00Z","updatedAt":"2026-01-02T00:00:00Z","author":null,
        "labels":{"totalCount":1,"nodes":[{"name":"bug","color":"abcdef"}]},
        "assignees":{"totalCount":1,"nodes":[{"login":"someone"}]},"comments":{"totalCount":4}})
}
pub(super) fn response(
    name: &str,
    nodes: Vec<Value>,
    total: usize,
    cursor: Option<&str>,
    issues: bool,
) -> Value {
    let mut repo = json!({"id":format!("R_{name}"),"name":name,"nameWithOwner":format!("owner/{name}"),"isArchived":false});
    repo[if issues { "issues" } else { "pullRequests" }] = connection(nodes, total, cursor);
    json!({"data":{"repository":repo}})
}
async fn mount(server: &MockServer, name: &str, cursor: Option<&str>, body: Value) {
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(body_partial_json(
            json!({"variables":{"name":name,"cursor":cursor}}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(server)
        .await;
}
fn scope(name: &str) -> RepositoryCoordinates {
    RepositoryCoordinates::new("owner", name).unwrap()
}
#[tokio::test]
async fn issues_paginate_and_keep_complete_metadata_and_repository_identity() {
    let server = MockServer::start().await;
    mount(
        &server,
        "repo",
        None,
        response(
            "repo",
            (0..100).map(item).collect(),
            101,
            Some("next"),
            true,
        ),
    )
    .await;
    mount(
        &server,
        "repo",
        Some("next"),
        response("repo", vec![item(100)], 101, None, true),
    )
    .await;
    let mut sizes = vec![];
    let items = GitHubService::with_client(client(&server, true))
        .get_open_issues_with_progress(&[scope("repo")], async |page| {
            sizes.push(page.items.len());
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(sizes, [100, 1]);
    assert_eq!(items.len(), 101);
    let issue = &items[0];
    assert_eq!(issue.repository.name_with_owner, "owner/repo");
    assert!(issue.author.is_none());
    assert_eq!(issue.comment_count, 4);
    assert_eq!(issue.labels[0].name, "bug");
    assert_eq!(issue.assignees[0].login, "someone");
    let requests = server.received_requests().await.unwrap();
    let body = requests[0].body_json::<Value>().unwrap();
    assert_eq!(body["variables"]["issues"], true);
    assert!(
        body["query"]
            .as_str()
            .unwrap()
            .contains("followRenames: false")
    );
}
#[tokio::test]
async fn pull_requests_preserve_drafts_review_decisions_and_sort_by_update() {
    let server = MockServer::start().await;
    let mut first = item(1);
    first["isDraft"] = json!(false);
    first["reviewDecision"] = json!("CHANGES_REQUESTED");
    let mut second = item(2);
    second["isDraft"] = json!(true);
    second["reviewDecision"] = Value::Null;
    second["updatedAt"] = json!("2026-02-01T00:00:00Z");
    mount(
        &server,
        "repo",
        None,
        response("repo", vec![first, second], 2, None, false),
    )
    .await;
    let items = GitHubService::with_client(client(&server, true))
        .get_open_pull_requests("owner", "repo")
        .await
        .unwrap();
    assert!(items[0].is_draft);
    assert!(items[0].review_decision.is_none());
    assert_eq!(
        items[1].review_decision,
        Some(ReviewDecision::ChangesRequested)
    );
    assert_eq!(
        server.received_requests().await.unwrap()[0]
            .body_json::<Value>()
            .unwrap()["variables"]["issues"],
        false
    );
}
#[tokio::test]
async fn truncated_metadata_and_missing_pr_fields_are_not_silent_successes() {
    for field in ["labels", "assignees", "isDraft"] {
        let server = MockServer::start().await;
        let mut node = item(0);
        if field != "isDraft" {
            node[field]["totalCount"] = json!(101);
            node["isDraft"] = json!(false);
        }
        mount(
            &server,
            "repo",
            None,
            response("repo", vec![node], 1, None, false),
        )
        .await;
        let error = GitHubService::with_client(client(&server, true))
            .get_open_pull_requests("owner", "repo")
            .await
            .unwrap_err();
        assert!(
            matches!(
                error,
                GitHubError::PaginationError(_) | GitHubError::ParseError(_)
            ),
            "{field}: {error}"
        );
    }
}
#[tokio::test]
async fn transferred_renamed_archived_and_recreated_repositories_are_rejected() {
    for field in ["id", "nameWithOwner", "isArchived"] {
        let server = MockServer::start().await;
        mount(
            &server,
            "repo",
            None,
            response("repo", vec![item(0)], 2, Some("next"), true),
        )
        .await;
        let mut body = response("repo", vec![item(1)], 2, None, true);
        body["data"]["repository"][field] = if field == "isArchived" {
            json!(true)
        } else {
            json!("changed")
        };
        mount(&server, "repo", Some("next"), body).await;
        let error = GitHubService::with_client(client(&server, true))
            .get_open_issues("owner", "repo")
            .await
            .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Pagination, "{field}");
    }
}
#[tokio::test]
async fn batches_validate_all_scopes_before_network_and_do_not_rediscover_ownership() {
    let server = MockServer::start().await;
    let service = GitHubService::with_client(client(&server, true));
    for scopes in [
        vec![scope("repo"), scope("REPO")],
        vec![
            scope("repo"),
            RepositoryCoordinates {
                owner: "owner".into(),
                name: "../bad".into(),
            },
        ],
    ] {
        assert_eq!(
            service
                .get_open_issues_for_repositories(&scopes)
                .await
                .unwrap_err()
                .kind(),
            ErrorKind::InvalidInput
        );
    }
    assert!(
        service
            .get_open_issues_for_repositories(&[])
            .await
            .unwrap()
            .is_empty()
    );
    assert!(server.received_requests().await.unwrap().is_empty());
    mount(
        &server,
        "repo",
        None,
        response("repo", vec![], 0, None, true),
    )
    .await;
    assert!(
        service
            .get_open_issues_for_repositories(&[scope("repo")])
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}
#[tokio::test]
async fn concurrency_and_callback_backpressure_bound_repository_requests() {
    let server = MockServer::start().await;
    let started = Arc::new(AtomicUsize::new(0));
    for name in ["a", "b", "c"] {
        let started = Arc::clone(&started);
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .and(body_partial_json(json!({"variables":{"name":name}})))
            .respond_with(move |_: &wiremock::Request| {
                started.fetch_add(1, Ordering::SeqCst);
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_millis(50))
                    .set_body_json(response(name, vec![], 0, None, true))
            })
            .expect(1)
            .mount(&server)
            .await;
    }
    let service = GitHubService::with_client(client(&server, true))
        .with_fetch_options(FetchOptions {
            max_concurrent_repositories: 2,
            ..Default::default()
        })
        .unwrap();
    let mut callbacks = 0;
    service
        .get_open_issues_with_progress(&[scope("a"), scope("b"), scope("c")], async |_| {
            callbacks += 1;
            if callbacks == 1 {
                assert_eq!(started.load(Ordering::SeqCst), 2);
                tokio::time::sleep(Duration::from_millis(80)).await;
                assert_eq!(
                    started.load(Ordering::SeqCst),
                    2,
                    "third request must wait for callback"
                );
            }
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(callbacks, 3);
    assert_eq!(started.load(Ordering::SeqCst), 3);
}
#[tokio::test]
async fn cancellation_stops_other_repositories_and_followup_pages() {
    let server = MockServer::start().await;
    mount(
        &server,
        "a",
        None,
        response("a", vec![item(0)], 2, Some("next"), true),
    )
    .await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(body_partial_json(json!({"variables":{"name":"b"}})))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(150))
                .set_body_json(response("b", vec![item(1)], 2, Some("next"), true)),
        )
        // Cancellation may drop this request before it reaches the server.
        .expect(0..=1)
        .mount(&server)
        .await;
    let service = GitHubService::with_client(client(&server, true))
        .with_fetch_options(FetchOptions {
            max_concurrent_repositories: 2,
            ..Default::default()
        })
        .unwrap();
    let error = service
        .get_open_issues_with_progress(&[scope("a"), scope("b"), scope("c")], async |_| {
            Err(GitHubError::Cancelled)
        })
        .await
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Cancelled);
    tokio::time::sleep(Duration::from_millis(200)).await;
    let requests = server.received_requests().await.unwrap();
    assert!(requests.len() <= 2);
    for request in requests {
        assert_eq!(request.method.as_str(), "POST");
        assert_eq!(request.url.path(), "/graphql");
        let body = request.body_json::<Value>().unwrap();
        assert!(body["variables"]["cursor"].is_null());
        assert_ne!(body["variables"]["name"], "c");
    }
}
#[tokio::test]
async fn page_caps_prevent_partial_success() {
    let server = MockServer::start().await;
    mount(
        &server,
        "repo",
        None,
        response("repo", vec![item(0)], 2, Some("next"), true),
    )
    .await;
    let service = GitHubService::with_client(client(&server, true))
        .with_fetch_options(FetchOptions {
            max_pages: 1,
            ..Default::default()
        })
        .unwrap();
    let mut callbacks = 0;
    let error = service
        .get_open_issues_with_progress(&[scope("repo")], async |_| {
            callbacks += 1;
            Ok(())
        })
        .await
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Pagination);
    assert_eq!(callbacks, 0);
}

#[tokio::test]
async fn buffered_issues_collect_every_page() {
    let server = MockServer::start().await;
    mount(
        &server,
        "repo",
        None,
        response("repo", vec![item(0)], 2, Some("next"), true),
    )
    .await;
    mount(
        &server,
        "repo",
        Some("next"),
        response("repo", vec![item(1)], 2, None, true),
    )
    .await;
    let issues = GitHubService::with_client(client(&server, true))
        .get_open_issues("owner", "repo")
        .await
        .unwrap();
    assert_eq!(issues.len(), 2);
    assert_eq!(issues[1].node_id, "I_1");
    assert_eq!(issues[1].labels[0].name, "bug");
}

#[tokio::test]
async fn empty_scopes_are_anonymous_no_ops_and_progress_futures_are_send() {
    let server = MockServer::start().await;
    let service = GitHubService::with_client(client(&server, false));
    assert!(
        service
            .get_open_issues_for_repositories(&[])
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        service
            .get_open_pull_requests_with_progress(&[], async |_| {
                panic!("empty scopes must not call progress")
            })
            .await
            .unwrap()
            .is_empty()
    );
    assert!(server.received_requests().await.unwrap().is_empty());
    assert_eq!(
        service
            .get_open_issues_for_repositories(&[scope("repo")])
            .await
            .unwrap_err()
            .kind(),
        ErrorKind::Authentication
    );
    mount(
        &server,
        "repo",
        None,
        response("repo", vec![item(0)], 1, None, true),
    )
    .await;
    let service = GitHubService::with_client(client(&server, true));
    let scopes = [scope("repo")];
    let ids = Arc::new(std::sync::Mutex::new(vec![]));
    let captured = Arc::clone(&ids);
    let operation = service.get_open_issues_with_progress(&scopes, async move |page| {
        tokio::task::yield_now().await;
        captured.lock().unwrap().push(page.items[0].node_id.clone());
        Ok(())
    });
    fn require_send<T: Send>(value: T) -> T {
        value
    }
    let issues = require_send(operation).await.unwrap();
    assert_eq!(*ids.lock().unwrap(), [issues[0].node_id.as_str()]);
}
