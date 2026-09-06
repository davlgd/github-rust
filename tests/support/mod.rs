use github_rust::GitHubClient;
use serde_json::{Value, json};
use wiremock::MockServer;

pub fn client(server: &MockServer, authenticated: bool) -> GitHubClient {
    let mut builder = GitHubClient::builder()
        .rest_url(server.uri())
        .graphql_url(format!("{}/graphql", server.uri()));
    if authenticated {
        builder = builder.token("test_token".into());
    }
    builder.build().unwrap()
}

pub fn rest_repository() -> Value {
    json!({
        "id": 1296269, "node_id": "opaque-node-id", "name": "repo", "full_name": "owner/repo",
        "html_url": "https://github.com/owner/repo", "description": null, "homepage": null,
        "created_at": "2026-01-01T00:00:00Z", "updated_at": "2026-01-02T00:00:00Z", "pushed_at": null,
        "private": false, "fork": false, "archived": false,
        "stargazers_count": 80, "watchers_count": 80, "subscribers_count": 3,
        "forks_count": 2, "open_issues_count": 17, "language": "Rust", "license": null,
        "default_branch": "main", "topics": ["rust"]
    })
}

pub fn graphql_repository() -> Value {
    json!({
        "id": "opaque-node-id", "databaseId": 1296269, "name": "repo", "nameWithOwner": "owner/repo",
        "url": "https://github.com/owner/repo", "description": null, "homepageUrl": null,
        "createdAt": "2026-01-01T00:00:00Z", "updatedAt": "2026-01-02T00:00:00Z", "pushedAt": null,
        "isPrivate": false, "isFork": false, "isArchived": false,
        "stargazerCount": 80, "watchers": {"totalCount": 3}, "forkCount": 2,
        "issues": {"totalCount": 12}, "pullRequests": {"totalCount": 25}, "releases": {"totalCount": 0},
        "primaryLanguage": {"name": "Rust", "color": null}, "licenseInfo": null,
        "defaultBranchRef": {"name": "main"},
        "languages": {"edges": [{"node": {"name": "Rust", "color": null}, "size": 100}], "pageInfo": {"hasNextPage": false}},
        "repositoryTopics": {"edges": [{"node": {"topic": {"name": "rust"}}}]}
    })
}
