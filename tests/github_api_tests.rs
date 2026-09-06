#[path = "cases/accounts.rs"]
mod accounts;
#[path = "cases/client.rs"]
mod client;
#[path = "cases/errors.rs"]
mod errors;
#[path = "cases/fallback.rs"]
mod fallback;
#[path = "cases/page_streams.rs"]
mod page_streams;
#[path = "cases/repository.rs"]
mod repository;
#[path = "cases/search.rs"]
mod search;
mod support;
#[path = "cases/work_items.rs"]
mod work_items;

use github_rust::{GitHubService, StargazerWithDate, User};

#[tokio::test]
#[ignore] // Only run with internet connection
async fn test_real_github_api_rate_limit() {
    let limits = GitHubService::new()
        .expect("Client construction failed")
        .check_rate_limit()
        .await
        .expect("Live rate limit request failed");
    assert!(limits.limit > 0);
    assert!(limits.remaining <= limits.limit);
}

#[test]
fn test_stargazer_types_serialization() {
    use serde_json;

    // Test User deserialization
    let user_json = r#"{
        "login": "testuser",
        "id": 12345,
        "node_id": "MDQ6VXNlcjEyMzQ1",
        "avatar_url": "https://avatars.githubusercontent.com/u/12345?v=4",
        "gravatar_id": "",
        "url": "https://api.github.com/users/testuser",
        "html_url": "https://github.com/testuser",
        "followers_url": "https://api.github.com/users/testuser/followers",
        "following_url": "https://api.github.com/users/testuser/following{/other_user}",
        "gists_url": "https://api.github.com/users/testuser/gists{/gist_id}",
        "starred_url": "https://api.github.com/users/testuser/starred{/owner}{/repo}",
        "subscriptions_url": "https://api.github.com/users/testuser/subscriptions",
        "organizations_url": "https://api.github.com/users/testuser/orgs",
        "repos_url": "https://api.github.com/users/testuser/repos",
        "events_url": "https://api.github.com/users/testuser/events{/privacy}",
        "received_events_url": "https://api.github.com/users/testuser/received_events",
        "type": "User",
        "site_admin": false
    }"#;

    let user: User = serde_json::from_str(user_json).expect("Failed to deserialize User");
    assert_eq!(user.login, "testuser");
    assert_eq!(user.id, 12345);
    assert_eq!(user.user_type, "User");
    assert!(!user.site_admin);

    // Test StargazerWithDate deserialization
    let stargazer_json = r#"{
        "starred_at": "2015-09-11T10:42:05Z",
        "user": {
            "login": "testuser",
            "id": 12345,
            "node_id": "MDQ6VXNlcjEyMzQ1",
            "avatar_url": "https://avatars.githubusercontent.com/u/12345?v=4",
            "gravatar_id": "",
            "url": "https://api.github.com/users/testuser",
            "html_url": "https://github.com/testuser",
            "followers_url": "https://api.github.com/users/testuser/followers",
            "following_url": "https://api.github.com/users/testuser/following{/other_user}",
            "gists_url": "https://api.github.com/users/testuser/gists{/gist_id}",
            "starred_url": "https://api.github.com/users/testuser/starred{/owner}{/repo}",
            "subscriptions_url": "https://api.github.com/users/testuser/subscriptions",
            "organizations_url": "https://api.github.com/users/testuser/orgs",
            "repos_url": "https://api.github.com/users/testuser/repos",
            "events_url": "https://api.github.com/users/testuser/events{/privacy}",
            "received_events_url": "https://api.github.com/users/testuser/received_events",
            "type": "User",
            "site_admin": false
        }
    }"#;

    let stargazer: StargazerWithDate =
        serde_json::from_str(stargazer_json).expect("Failed to deserialize StargazerWithDate");
    assert_eq!(stargazer.starred_at, "2015-09-11T10:42:05Z");
    assert_eq!(stargazer.user.login, "testuser");
    assert_eq!(stargazer.user.id, 12345);

    // Test serialization round-trip
    let serialized =
        serde_json::to_string(&stargazer).expect("Failed to serialize StargazerWithDate");
    let deserialized: StargazerWithDate = serde_json::from_str(&serialized)
        .expect("Failed to deserialize serialized StargazerWithDate");
    assert_eq!(deserialized.starred_at, stargazer.starred_at);
    assert_eq!(deserialized.user.login, stargazer.user.login);
}

#[tokio::test]
async fn test_stargazers_api_with_mock() {
    use github_rust::github::client::GitHubClient;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // Start a mock server
    let mock_server = MockServer::start().await;

    // Mock successful stargazers response
    let stargazers_response = r#"[
        {
            "starred_at": "2015-09-11T10:42:05Z",
            "user": {
                "login": "testuser1",
                "id": 12345,
                "node_id": "MDQ6VXNlcjEyMzQ1",
                "avatar_url": "https://avatars.githubusercontent.com/u/12345?v=4",
                "gravatar_id": "",
                "url": "https://api.github.com/users/testuser1",
                "html_url": "https://github.com/testuser1",
                "followers_url": "https://api.github.com/users/testuser1/followers",
                "following_url": "https://api.github.com/users/testuser1/following{/other_user}",
                "gists_url": "https://api.github.com/users/testuser1/gists{/gist_id}",
                "starred_url": "https://api.github.com/users/testuser1/starred{/owner}{/repo}",
                "subscriptions_url": "https://api.github.com/users/testuser1/subscriptions",
                "organizations_url": "https://api.github.com/users/testuser1/orgs",
                "repos_url": "https://api.github.com/users/testuser1/repos",
                "events_url": "https://api.github.com/users/testuser1/events{/privacy}",
                "received_events_url": "https://api.github.com/users/testuser1/received_events",
                "type": "User",
                "site_admin": false
            }
        },
        {
            "starred_at": "2015-09-12T15:30:00Z",
            "user": {
                "login": "testuser2",
                "id": 67890,
                "node_id": "MDQ6VXNlcjY3ODkw",
                "avatar_url": "https://avatars.githubusercontent.com/u/67890?v=4",
                "gravatar_id": "",
                "url": "https://api.github.com/users/testuser2",
                "html_url": "https://github.com/testuser2",
                "followers_url": "https://api.github.com/users/testuser2/followers",
                "following_url": "https://api.github.com/users/testuser2/following{/other_user}",
                "gists_url": "https://api.github.com/users/testuser2/gists{/gist_id}",
                "starred_url": "https://api.github.com/users/testuser2/starred{/owner}{/repo}",
                "subscriptions_url": "https://api.github.com/users/testuser2/subscriptions",
                "organizations_url": "https://api.github.com/users/testuser2/orgs",
                "repos_url": "https://api.github.com/users/testuser2/repos",
                "events_url": "https://api.github.com/users/testuser2/events{/privacy}",
                "received_events_url": "https://api.github.com/users/testuser2/received_events",
                "type": "User",
                "site_admin": false
            }
        }
    ]"#;

    Mock::given(method("GET"))
        .and(path("/repos/microsoft/vscode/stargazers"))
        .and(header("Accept", "application/vnd.github.v3.star+json"))
        .and(header("Authorization", "Bearer test_token"))
        .and(header("X-GitHub-Api-Version", "2022-11-28"))
        .and(query_param("per_page", "2"))
        .and(query_param("page", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_string(stargazers_response))
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = GitHubClient::builder()
        .rest_url(mock_server.uri())
        .graphql_url(format!("{}/graphql", mock_server.uri()))
        .token("test_token".into())
        .http_client(reqwest::Client::new())
        .build()
        .unwrap();
    let service = GitHubService::with_client(client);
    let stargazers = service
        .get_repository_stargazers("microsoft", "vscode", Some(2), Some(1))
        .await
        .unwrap();
    assert_eq!(stargazers.len(), 2);
    assert_eq!(stargazers[0].user.login, "testuser1");
    assert_eq!(stargazers[0].starred_at, "2015-09-11T10:42:05Z");
}

#[tokio::test]
async fn test_stargazers_api_error_handling() {
    use github_rust::github::client::GitHubClient;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // Start a mock server
    let mock_server = MockServer::start().await;

    // Mock 404 response
    Mock::given(method("GET"))
        .and(path("/repos/nonexistent/repo/stargazers"))
        .respond_with(ResponseTemplate::new(404).set_body_string("Not Found"))
        .expect(1)
        .mount(&mock_server)
        .await;

    // Mock 403 response (rate limit)
    Mock::given(method("GET"))
        .and(path("/repos/ratelimited/repo/stargazers"))
        .respond_with(ResponseTemplate::new(403).set_body_string("API rate limit exceeded"))
        .expect(1)
        .mount(&mock_server)
        .await;

    // Mock 401 response (authentication)
    Mock::given(method("GET"))
        .and(path("/repos/private/repo/stargazers"))
        .respond_with(ResponseTemplate::new(401).set_body_string("Bad credentials"))
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = GitHubClient::builder()
        .rest_url(mock_server.uri())
        .build()
        .unwrap();
    let service = GitHubService::with_client(client);
    use github_rust::GitHubError;
    assert!(matches!(
        service
            .get_repository_stargazers("nonexistent", "repo", None, None)
            .await,
        Err(GitHubError::NotFoundError(_))
    ));
    assert!(matches!(
        service
            .get_repository_stargazers("ratelimited", "repo", None, None)
            .await,
        Err(GitHubError::RateLimitError(_))
    ));
    assert!(matches!(
        service
            .get_repository_stargazers("private", "repo", None, None)
            .await,
        Err(GitHubError::AuthenticationError(_))
    ));
}
