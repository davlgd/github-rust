use crate::support::client;
use github_rust::{
    GitHubError,
    github::{graphql, rest},
};
use serde_json::json;
use std::error::Error;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};

#[tokio::test]
async fn rate_limit_checks_reject_http_and_schema_errors() {
    for (status, body) in [
        (401, json!({"message": "Bad credentials"})),
        (500, json!({"message": "Server error"})),
        (200, json!({"resources": {"core": {"limit": 5000}}})),
    ] {
        let server = MockServer::start().await;
        Mock::given(path("/rate_limit"))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .expect(1)
            .mount(&server)
            .await;
        let error = client(&server, false).check_rate_limit().await.unwrap_err();
        match status {
            401 => assert!(matches!(error, GitHubError::AuthenticationError(_))),
            500 => assert!(matches!(error, GitHubError::ApiError { status: 500, .. })),
            _ => {
                assert!(matches!(error, GitHubError::ParseError(_)));
                assert!(error.source().is_some());
            }
        }
    }
}

#[tokio::test]
async fn separate_quotas_are_preserved() {
    let server = MockServer::start().await;
    Mock::given(path("/rate_limit"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"resources": {
                "core": {"limit": 5000, "remaining": 4500, "reset": 1800000000u64},
                "graphql": {"limit": 5000, "remaining": 0, "reset": 1800001000u64},
                "search": {"limit": 30, "remaining": 20, "reset": 1800002000u64}
            }})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let limits = client(&server, true).check_rate_limits().await.unwrap();
    assert_eq!(limits.resources["core"].remaining, 4500);
    assert!(limits.resources["graphql"].is_exceeded());
    assert_eq!(limits.resources["search"].limit, 30);
}

#[tokio::test]
async fn rate_limit_headers_survive_403_and_429() {
    for status in [403, 429] {
        let server = MockServer::start().await;
        Mock::given(path("/rate_limit"))
            .respond_with(
                ResponseTemplate::new(status)
                    .set_body_json(json!({"message": "Slow down"}))
                    .insert_header("Retry-After", "60")
                    .insert_header("X-RateLimit-Remaining", "0")
                    .insert_header("X-RateLimit-Reset", "1800000000")
                    .insert_header("X-RateLimit-Resource", "core")
                    .insert_header("X-GitHub-Request-Id", "request-123"),
            )
            .expect(1)
            .mount(&server)
            .await;
        let error = client(&server, false).check_rate_limit().await.unwrap_err();
        let GitHubError::RateLimitError(details) = error else {
            panic!("{error:?}");
        };
        assert_eq!(details.status, Some(status));
        assert_eq!(details.retry_after.as_deref(), Some("60"));
        assert_eq!(details.reset, Some(1800000000));
        assert_eq!(details.resource.as_deref(), Some("core"));
        assert_eq!(details.request_id.as_deref(), Some("request-123"));
    }
}

#[tokio::test]
async fn permission_denial_is_not_rate_limiting() {
    let server = MockServer::start().await;
    Mock::given(path("/user"))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_json(json!({"message": "Resource not accessible by integration"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    assert!(matches!(
        rest::get_user_profile(&client(&server, true)).await,
        Err(GitHubError::AccessDeniedError(_))
    ));
}

#[tokio::test]
async fn malformed_json_keeps_its_decoding_cause() {
    let server = MockServer::start().await;
    Mock::given(path("/rate_limit"))
        .respond_with(ResponseTemplate::new(200).set_body_string("{broken"))
        .expect(1)
        .mount(&server)
        .await;
    let error = client(&server, false).check_rate_limit().await.unwrap_err();
    assert!(matches!(error, GitHubError::ParseError(_)));
    assert!(error.source().unwrap().source().is_some());
}

#[tokio::test]
async fn graphql_errors_keep_types_and_rate_metadata() {
    let server = MockServer::start().await;
    Mock::given(path("/graphql"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"errors": [
                    {"type": "RATE_LIMITED", "message": "Quota exhausted", "path": ["repository"]}
                ]}))
                .insert_header("X-RateLimit-Reset", "1800000000"),
        )
        .expect(1)
        .mount(&server)
        .await;
    let error = graphql::get_repository_info(&client(&server, true), "owner", "repo")
        .await
        .unwrap_err();
    let GitHubError::RateLimitError(details) = error else {
        panic!("{error:?}");
    };
    assert_eq!(details.resource.as_deref(), Some("graphql"));
    assert_eq!(
        details.graphql_errors[0].error_type.as_deref(),
        Some("RATE_LIMITED")
    );
    assert_eq!(details.reset, Some(1800000000));
}

#[tokio::test]
async fn not_found_errors_preserve_the_requested_repository() {
    let server = MockServer::start().await;
    for endpoint in [
        "/repos/owner/repo",
        "/repos/owner/repo/stargazers",
        "/graphql",
    ] {
        Mock::given(path(endpoint))
            .respond_with(ResponseTemplate::new(404).set_body_json(json!({"message": "Not Found"})))
            .expect(1)
            .mount(&server)
            .await;
    }
    let client = client(&server, true);
    let errors = [
        rest::get_repository_info(&client, "owner", "repo")
            .await
            .unwrap_err(),
        rest::get_repository_stargazers(&client, "owner", "repo", None, None)
            .await
            .unwrap_err(),
        graphql::get_repository_info(&client, "owner", "repo")
            .await
            .unwrap_err(),
    ];
    for error in errors {
        assert!(
            matches!(error, GitHubError::NotFoundError(ref repository) if repository == "owner/repo")
        );
    }
}
