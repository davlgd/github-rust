use crate::support::*;
use github_rust::github::{graphql, rest};
use serde_json::json;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

#[tokio::test]
async fn transports_preserve_identity_and_distinguish_unknown_counts() {
    let server = MockServer::start().await;
    let mut rest_body = rest_repository();
    rest_body["license"] = json!({"name": "MIT License", "spdx_id": "MIT"});
    let mut graphql_body = graphql_repository();
    graphql_body["licenseInfo"] = json!({"name": "MIT License", "spdxId": "MIT"});
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo"))
        .respond_with(ResponseTemplate::new(200).set_body_json(rest_body))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo/languages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"Rust": 100})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(|request: &wiremock::Request| {
            let body: serde_json::Value = request.body_json().unwrap();
            body["query"]
                .as_str()
                .unwrap()
                .contains("issues(states: OPEN)")
        })
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"data": {"repository": graphql_body}})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let client = client(&server, true);
    let rest = rest::get_repository_info(&client, "owner", "repo")
        .await
        .unwrap();
    let graphql = graphql::get_repository_info(&client, "owner", "repo")
        .await
        .unwrap();
    assert_eq!(rest.node_id, graphql.node_id);
    assert_eq!(rest.database_id, Some(1296269));
    assert_eq!(rest.database_id, graphql.database_id);
    assert_eq!(rest.watcher_count(), Some(3));
    assert_eq!(rest.watcher_count(), graphql.watcher_count());
    assert_eq!(rest.open_issues(), None);
    assert_eq!(graphql.open_issues(), Some(12));
    assert_eq!(rest.pull_request_count, None);
    assert_eq!(graphql.pull_request_count, Some(25));
    assert_eq!(rest.release_count, None);
    assert_eq!(graphql.release_count, Some(0));
    assert_eq!(rest.topics(), graphql.topics());
    assert!(rest.languages_complete && graphql.languages_complete);
    for repo in [rest, graphql] {
        assert_eq!(repo.license_spdx(), Some("MIT"));
        let serialized = serde_json::to_value(repo).unwrap();
        assert_eq!(serialized["node_id"], "opaque-node-id");
        assert!(serialized.get("repositoryTopics").is_none());
        assert_eq!(serialized["license_info"]["spdxId"], "MIT");
        assert!(serialized["license_info"].get("spdx_id").is_none());
        let restored: github_rust::Repository = serde_json::from_value(serialized).unwrap();
        assert_eq!(restored.license_spdx(), Some("MIT"));
    }
}

#[tokio::test]
async fn missing_languages_are_not_an_empty_successful_breakdown() {
    let server = MockServer::start().await;
    Mock::given(path("/repos/owner/repo"))
        .respond_with(ResponseTemplate::new(200).set_body_json(rest_repository()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/repos/owner/repo/languages"))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&server)
        .await;
    let repo = rest::get_repository_info(&client(&server, false), "owner", "repo")
        .await
        .unwrap();
    assert!(repo.languages.is_none());
    assert!(!repo.languages_complete);
}

#[tokio::test]
async fn graphql_language_truncation_is_explicit() {
    let server = MockServer::start().await;
    let mut repo = graphql_repository();
    repo["languages"]["pageInfo"]["hasNextPage"] = json!(true);
    Mock::given(path("/graphql"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"data": {"repository": repo}})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let repo = graphql::get_repository_info(&client(&server, true), "owner", "repo")
        .await
        .unwrap();
    assert!(repo.languages.is_some());
    assert!(!repo.languages_complete);
}
