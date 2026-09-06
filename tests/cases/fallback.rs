use crate::support::*;
use github_rust::{FallbackPolicy, GitHubError, GitHubService};
use serde_json::json;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};

async fn rest_success(server: &MockServer) {
    Mock::given(path("/repos/owner/repo"))
        .respond_with(ResponseTemplate::new(200).set_body_json(rest_repository()))
        .expect(1)
        .mount(server)
        .await;
    Mock::given(path("/repos/owner/repo/languages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(server)
        .await;
}

#[tokio::test]
async fn anonymous_lookup_uses_rest_directly() {
    let server = MockServer::start().await;
    rest_success(&server).await;
    let service = GitHubService::with_client(client(&server, false));
    assert_eq!(
        service
            .get_repository_info("owner", "repo")
            .await
            .unwrap()
            .node_id,
        "opaque-node-id"
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn server_unavailable_can_fall_back_to_rest() {
    let server = MockServer::start().await;
    rest_success(&server).await;
    Mock::given(path("/graphql"))
        .respond_with(ResponseTemplate::new(503))
        .expect(1)
        .mount(&server)
        .await;
    GitHubService::with_client(client(&server, true))
        .get_repository_info("owner", "repo")
        .await
        .unwrap();
}

#[tokio::test]
async fn permanent_and_decoding_errors_do_not_trigger_fallback() {
    for (status, body) in [
        (401, "{}"),
        (403, "{}"),
        (404, "{}"),
        (429, "{}"),
        (451, "{}"),
        (500, "{}"),
        (200, "not-json"),
        (
            200,
            r#"{"errors":[{"type":"NOT_FOUND","message":"Missing"}]}"#,
        ),
    ] {
        let server = MockServer::start().await;
        Mock::given(path("/graphql"))
            .respond_with(ResponseTemplate::new(status).set_body_string(body))
            .expect(1)
            .mount(&server)
            .await;
        assert!(
            GitHubService::with_client(client(&server, true))
                .get_repository_info("owner", "repo")
                .await
                .is_err()
        );
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }
}

#[tokio::test]
async fn fallback_can_be_disabled() {
    let server = MockServer::start().await;
    Mock::given(path("/graphql"))
        .respond_with(ResponseTemplate::new(503))
        .expect(1)
        .mount(&server)
        .await;
    let service = GitHubService::with_client(client(&server, true))
        .with_fallback_policy(FallbackPolicy::Never);
    assert!(matches!(
        service.get_repository_info("owner", "repo").await,
        Err(GitHubError::ApiError { status: 503, .. })
    ));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn fallback_failure_preserves_both_causes() {
    let server = MockServer::start().await;
    Mock::given(path("/graphql"))
        .respond_with(ResponseTemplate::new(503).set_body_string("Unavailable"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/repos/owner/repo"))
        .respond_with(ResponseTemplate::new(404).set_body_string("Missing"))
        .expect(1)
        .mount(&server)
        .await;
    let error = GitHubService::with_client(client(&server, true))
        .get_repository_info("owner", "repo")
        .await
        .unwrap_err();
    let GitHubError::FallbackError { graphql, rest } = error else {
        panic!("{error:?}");
    };
    assert!(matches!(
        *graphql,
        GitHubError::ApiError { status: 503, .. }
    ));
    assert!(matches!(*rest, GitHubError::NotFoundError(_)));
}
