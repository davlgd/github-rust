use crate::support::{client, graphql_repository};
use github_rust::{
    GitHubError,
    github::{rest, search},
};
use serde_json::{Value, json};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, query_param},
};

fn page(count: usize, has_next: bool, cursor: Value) -> Value {
    json!({"data": {"search": {"repositoryCount": 1000,
        "pageInfo": {"hasNextPage": has_next, "endCursor": cursor},
        "edges": (0..count).map(|_| json!({"node": graphql_repository()})).collect::<Vec<_>>()
    }}, "errors": []})
}

#[tokio::test]
async fn zero_limits_and_invalid_inputs_do_not_send_requests() {
    let server = MockServer::start().await;
    let client = client(&server, false);
    assert!(
        search::search_repositories(&client, 30, 0, None, 0)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        search::search_repositories(&client, u32::MAX, 10, None, 0).await,
        Err(GitHubError::InvalidInput(_))
    ));
    assert!(matches!(
        search::search_repositories(&client, 30, 10, Some("Rust\" repo:private"), 0).await,
        Err(GitHubError::InvalidInput(_))
    ));
    assert!(matches!(
        search::search_repositories(&client, 30, 10, None, 0).await,
        Err(GitHubError::AuthenticationError(_))
    ));
    for (per_page, page) in [(Some(0), None), (None, Some(0))] {
        assert!(matches!(
            rest::get_repository_stargazers(&client, "owner", "repo", per_page, page).await,
            Err(GitHubError::InvalidInput(_))
        ));
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn search_quotes_languages_and_requests_only_remaining_results() {
    let server = MockServer::start().await;
    for (after, first, count, has_next, end) in [
        (Value::Null, 100, 100, true, json!("next")),
        (json!("next"), 5, 5, false, Value::Null),
    ] {
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .and(move |request: &wiremock::Request| {
                let body: Value = request.body_json().unwrap();
                body["variables"]["first"] == first
                    && body["variables"]["after"] == after
                    && body["variables"]["queryString"]
                        .as_str()
                        .unwrap()
                        .contains("language:\"Visual Basic .NET\"")
            })
            .respond_with(ResponseTemplate::new(200).set_body_json(page(count, has_next, end)))
            .expect(1)
            .mount(&server)
            .await;
    }
    let repos = search::search_repositories(
        &client(&server, true),
        30,
        105,
        Some("Visual Basic .NET"),
        0,
    )
    .await
    .unwrap();
    assert_eq!(repos.len(), 105);
    assert_eq!(repos[0].node_id, "opaque-node-id");
    assert_eq!(repos[0].database_id, Some(1296269));
    assert_eq!(repos[0].topics(), ["rust"]);
}

#[tokio::test]
async fn search_rejects_missing_repeated_and_nonprogressing_cursors() {
    for (count, cursor, expected_requests) in [
        (1, Value::Null, 1),
        (1, json!("stuck"), 2),
        (0, json!("next"), 1),
    ] {
        let server = MockServer::start().await;
        Mock::given(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page(count, true, cursor)))
            .expect(expected_requests)
            .mount(&server)
            .await;
        assert!(matches!(
            search::search_repositories(&client(&server, true), 30, 10, None, 0).await,
            Err(GitHubError::PaginationError(_))
        ));
    }
}

#[tokio::test]
async fn search_caps_results_at_one_thousand() {
    let server = MockServer::start().await;
    Mock::given(path("/graphql"))
        .respond_with(|request: &wiremock::Request| {
            let body: Value = request.body_json().unwrap();
            let cursor = body["variables"]["after"]
                .as_str()
                .unwrap_or("0")
                .parse::<usize>()
                .unwrap();
            assert_eq!(body["variables"]["first"], 100);
            ResponseTemplate::new(200).set_body_json(page(
                100,
                true,
                json!((cursor + 1).to_string()),
            ))
        })
        .expect(10)
        .mount(&server)
        .await;
    assert_eq!(
        search::search_repositories(&client(&server, true), 30, usize::MAX, None, 0)
            .await
            .unwrap()
            .len(),
        1000
    );
}

#[tokio::test]
async fn stargazers_encode_paths_and_cap_page_size() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/owner%2Fname/repo%3Fname/stargazers"))
        .and(query_param("per_page", "100"))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(&server)
        .await;
    rest::get_repository_stargazers(
        &client(&server, false),
        "owner/name",
        "repo?name",
        Some(150),
        Some(2),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn starred_repositories_do_not_silently_truncate() {
    let server = MockServer::start().await;
    Mock::given(path("/user/starred"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(vec![json!({"full_name": "owner/repo"}); 100]),
        )
        .expect(100)
        .mount(&server)
        .await;
    assert!(matches!(
        rest::get_user_starred_repositories(&client(&server, true)).await,
        Err(GitHubError::PaginationError(_))
    ));
}
