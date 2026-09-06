use crate::support::client;
use github_rust::{
    ErrorKind, GitHubError, GitHubService,
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

#[tokio::test]
async fn forbidden_responses_with_quota_headers_are_rate_limits() {
    for (header, value) in [("X-RateLimit-Remaining", "0"), ("Retry-After", "30")] {
        let server = MockServer::start().await;
        Mock::given(path("/user"))
            .respond_with(
                ResponseTemplate::new(403)
                    .set_body_json(json!({"message": "Forbidden"}))
                    .insert_header(header, value),
            )
            .expect(1)
            .mount(&server)
            .await;
        // UserProfile does not implement Debug, so unwrap_err is unavailable.
        let Err(error) = rest::get_user_profile(&client(&server, true)).await else {
            panic!("{header}: expected a rate limit error");
        };
        assert!(
            matches!(error, GitHubError::RateLimitError(_)),
            "{header}: {error}"
        );
        assert_eq!(error.kind(), ErrorKind::RateLimit);
    }
}

#[tokio::test]
async fn blocked_repositories_are_typed_permission_errors() {
    for (status, message, expected) in [
        (403, "Repository access blocked", "AccessBlocked"),
        (
            451,
            "Repository access blocked due to a DMCA takedown",
            "Dmca",
        ),
    ] {
        let server = MockServer::start().await;
        Mock::given(path("/repos/owner/repo"))
            .respond_with(ResponseTemplate::new(status).set_body_json(json!({"message": message})))
            .expect(1)
            .mount(&server)
            .await;
        let error = rest::get_repository_info(&client(&server, false), "owner", "repo")
            .await
            .unwrap_err();
        match expected {
            "AccessBlocked" => assert!(matches!(error, GitHubError::AccessBlockedError(_))),
            _ => assert!(matches!(error, GitHubError::DmcaBlockedError(_))),
        }
        assert_eq!(error.kind(), ErrorKind::Permission, "{status}: {error}");
    }
}

#[tokio::test]
async fn fallback_errors_classify_the_rest_outcome_and_keep_the_graphql_cause() {
    let server = MockServer::start().await;
    Mock::given(path("/graphql"))
        .respond_with(ResponseTemplate::new(503).set_body_string("Unavailable"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/repos/owner/repo"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"message": "Not Found"})))
        .expect(1)
        .mount(&server)
        .await;
    let error = GitHubService::with_client(client(&server, true))
        .get_repository_info("owner", "repo")
        .await
        .unwrap_err();
    assert!(matches!(error, GitHubError::FallbackError { .. }));
    assert_eq!(error.kind(), ErrorKind::NotFound);
    let cause = error.source().unwrap().to_string();
    assert!(cause.contains("503"), "{cause}");
}

#[test]
fn graphql_error_kinds_follow_type_precedence() {
    fn errors(types: &[&str]) -> GitHubError {
        GitHubError::GraphQLError(
            types
                .iter()
                .map(|kind| serde_json::from_value(json!({"type": kind, "message": kind})).unwrap())
                .collect(),
        )
    }
    for (types, expected) in [
        (&["UNAUTHORIZED", "RATE_LIMITED"][..], ErrorKind::RateLimit),
        (&["FORBIDDEN", "UNAUTHENTICATED"], ErrorKind::Authentication),
        (&["NOT_FOUND", "INSUFFICIENT_SCOPES"], ErrorKind::Permission),
        (&["NOT_FOUND", "NOT_FOUND"], ErrorKind::NotFound),
        (&["NOT_FOUND", "SOMETHING_ELSE"], ErrorKind::Upstream),
        (&[], ErrorKind::Upstream),
    ] {
        assert_eq!(errors(types).kind(), expected, "{types:?}");
    }
}
