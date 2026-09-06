use crate::support::client;
use github_rust::{ErrorKind, FetchOptions, GitHubError, GitHubService};
use serde_json::{Value, json};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_partial_json, method, path},
};

pub fn account() -> Value {
    json!({"id":"U_owner", "login":"owner", "name":null, "avatarUrl":"https://avatars.example/owner"})
}
pub fn repository(n: usize) -> Value {
    json!({"id":format!("R_{n}"),"name":format!("repo{n}"),"nameWithOwner":format!("owner/repo{n}"),
        "description":null,"url":format!("https://github.com/owner/repo{n}"),"isPrivate":true,"isFork":true,"isArchived":true,
        "stargazerCount":3,"forkCount":1,"pushedAt":null,"updatedAt":"2026-01-01T00:00:00Z",
        "primaryLanguage":{"name":"Rust","color":null},"issues":{"totalCount":2},"pullRequests":{"totalCount":7}})
}
pub fn connection(nodes: Vec<Value>, total: usize, cursor: Option<&str>) -> Value {
    json!({"nodes":nodes,"totalCount":total,"pageInfo":{"hasNextPage":cursor.is_some(),"endCursor":cursor}})
}
async fn mount(server: &MockServer, cursor: Option<&str>, response: Value) {
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(body_partial_json(json!({"variables":{"cursor":cursor}})))
        .respond_with(ResponseTemplate::new(200).set_body_json(response))
        .expect(1)
        .mount(server)
        .await;
}
#[tokio::test]
async fn viewer_paginates_all_organizations() {
    let server = MockServer::start().await;
    for (cursor, nodes, next) in [
        (
            None,
            vec![json!({"id":"O_b","login":"b","name":null,"avatarUrl":"b"})],
            Some("next"),
        ),
        (
            Some("next"),
            vec![json!({"id":"O_a","login":"a","name":null,"avatarUrl":"a"})],
            None,
        ),
    ] {
        let mut viewer = account();
        viewer["organizations"] = connection(nodes, 2, next);
        mount(&server, cursor, json!({"data":{"viewer":viewer}})).await;
    }
    let viewer = GitHubService::with_client(client(&server, true))
        .get_viewer()
        .await
        .unwrap();
    assert_eq!(viewer.account.login, "owner");
    assert_eq!(
        viewer
            .organizations
            .iter()
            .map(|a| a.login.as_str())
            .collect::<Vec<_>>(),
        ["a", "b"]
    );
}
#[tokio::test]
async fn viewer_identity_changes_between_pages_are_rejected() {
    for field in ["id", "login"] {
        let server = MockServer::start().await;
        let organization = |login: &str| json!({"id":format!("O_{login}"),"login":login,"name":null,"avatarUrl":login});
        let mut viewer = account();
        viewer["organizations"] = connection(vec![organization("a")], 2, Some("next"));
        mount(&server, None, json!({"data":{"viewer":viewer}})).await;
        let mut viewer = account();
        viewer[field] = json!("changed");
        viewer["organizations"] = connection(vec![organization("b")], 2, None);
        mount(&server, Some("next"), json!({"data":{"viewer":viewer}})).await;
        let error = GitHubService::with_client(client(&server, true))
            .get_viewer()
            .await
            .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Pagination, "{field}: {error}");
        assert!(
            error.to_string().contains("Viewer changed"),
            "{field}: {error}"
        );
    }
}
#[tokio::test]
async fn owned_repositories_stream_over_one_hundred_without_losing_metadata() {
    let server = MockServer::start().await;
    for (cursor, nodes, next) in [
        (None, (0..100).map(repository).collect(), Some("next")),
        (Some("next"), vec![repository(100)], None),
    ] {
        let mut owner = account();
        owner["repositories"] = connection(nodes, 101, next);
        mount(&server, cursor, json!({"data":{"repositoryOwner":owner}})).await;
    }
    let mut sizes = vec![];
    let result = GitHubService::with_client(client(&server, true))
        .get_owned_repositories_with_progress("owner", async |page| {
            sizes.push(page.repositories.len());
            assert_eq!(page.total_count, 101);
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(sizes, [100, 1]);
    assert_eq!(result.repositories.len(), 101);
    let repo = &result.repositories[0];
    assert!(repo.is_private && repo.is_fork && repo.is_archived);
    assert_eq!(
        (repo.open_issue_count, repo.open_pull_request_count),
        (2, 7)
    );
    let requests = server.received_requests().await.unwrap();
    let query = requests[0].body_json::<Value>().unwrap()["query"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(query.contains("ownerAffiliations: [OWNER]"));
    assert!(query.contains("... on Organization"));
    assert!(query.contains("pullRequests(states: OPEN)"));
}
#[tokio::test]
async fn progress_can_cancel_before_the_next_request() {
    let server = MockServer::start().await;
    let mut owner = account();
    owner["repositories"] = connection(vec![repository(0)], 2, Some("next"));
    mount(&server, None, json!({"data":{"repositoryOwner":owner}})).await;
    let result = GitHubService::with_client(client(&server, true))
        .get_owned_repositories_with_progress("owner", async |_| Err(GitHubError::Cancelled))
        .await;
    assert_eq!(result.unwrap_err().kind(), ErrorKind::Cancelled);
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}
#[tokio::test]
async fn incomplete_or_unstable_pages_are_errors() {
    for mode in [
        "duplicate",
        "changed_total",
        "changed_owner",
        "missing_cursor",
        "empty_page",
        "short_final",
        "cursor_cycle",
    ] {
        let server = MockServer::start().await;
        let mut owner = account();
        owner["repositories"] = connection(vec![repository(0)], 3, Some("next"));
        mount(&server, None, json!({"data":{"repositoryOwner":owner}})).await;
        let mut owner = account();
        owner["repositories"] = connection(vec![repository(1)], 3, None);
        match mode {
            "duplicate" => owner["repositories"]["nodes"] = json!([repository(0), repository(2)]),
            "changed_total" => owner["repositories"]["totalCount"] = json!(2),
            "changed_owner" => owner["id"] = json!("different"),
            "missing_cursor" => owner["repositories"]["pageInfo"]["hasNextPage"] = json!(true),
            "empty_page" => owner["repositories"] = connection(vec![], 3, Some("third")),
            "cursor_cycle" => {
                owner["repositories"] = connection(vec![repository(1)], 3, Some("next"))
            }
            _ => {}
        }
        mount(
            &server,
            Some("next"),
            json!({"data":{"repositoryOwner":owner}}),
        )
        .await;
        let error = GitHubService::with_client(client(&server, true))
            .get_owned_repositories("owner")
            .await
            .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Pagination, "{mode}: {error}");
    }
}
#[tokio::test]
async fn collection_validation_and_graphql_error_precedence() {
    let server = MockServer::start().await;
    let service = GitHubService::with_client(client(&server, false));
    assert_eq!(
        service.get_viewer().await.unwrap_err().kind(),
        ErrorKind::Authentication
    );
    assert_eq!(
        service
            .get_owned_repositories("../bad")
            .await
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidInput
    );
    assert!(
        service
            .with_fetch_options(FetchOptions {
                page_size: 0,
                ..Default::default()
            })
            .is_err()
    );
    assert!(server.received_requests().await.unwrap().is_empty());
    mount(&server,None,json!({"data":{"viewer":{"broken":true}},"errors":[{"type":"FORBIDDEN","message":"private upstream detail"}]})).await;
    let error = GitHubService::with_client(client(&server, true))
        .get_viewer()
        .await
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Permission);
    assert!(matches!(error, GitHubError::GraphQLError(_)));
}

#[tokio::test]
async fn buffered_inventory_collects_every_page() {
    let server = MockServer::start().await;
    for (cursor, number, next) in [(None, 0, Some("next")), (Some("next"), 1, None)] {
        let mut owner = account();
        owner["repositories"] = connection(vec![repository(number)], 2, next);
        mount(&server, cursor, json!({"data": {"repositoryOwner": owner}})).await;
    }
    let inventory = GitHubService::with_client(client(&server, true))
        .get_owned_repositories("owner")
        .await
        .unwrap();
    assert_eq!(inventory.repositories.len(), 2);
    assert_eq!(inventory.repositories[1].node_id, "R_1");
    assert_eq!(inventory.repositories[1].open_pull_request_count, 7);
}

#[tokio::test]
async fn progress_can_borrow_the_page_and_mutable_state_across_awaits() {
    let server = MockServer::start().await;
    let mut owner = account();
    owner["repositories"] = connection(vec![repository(0)], 1, None);
    mount(&server, None, json!({"data": {"repositoryOwner": owner}})).await;
    let service = GitHubService::with_client(client(&server, true));
    let mut ids = vec![];
    let operation = service.get_owned_repositories_with_progress("owner", async |page| {
        tokio::task::yield_now().await;
        ids.push(page.repositories[0].node_id.clone());
        Ok(())
    });
    let inventory = operation.await.unwrap();
    assert_eq!(ids, [inventory.repositories[0].node_id.as_str()]);
}
