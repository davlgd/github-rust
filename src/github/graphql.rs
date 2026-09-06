use crate::github::client::GitHubClient;
use crate::github::types::*;
use crate::{config::*, error::*};
use serde::Deserialize;
use serde_json::json;

pub use crate::github::models::Repository;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphQLRepository {
    #[serde(rename = "id")]
    node_id: String,
    database_id: Option<u64>,
    name: String,
    name_with_owner: String,
    description: Option<String>,
    url: String,
    homepage_url: Option<String>,
    created_at: String,
    updated_at: String,
    pushed_at: Option<String>,
    is_private: bool,
    is_fork: bool,
    is_archived: bool,
    stargazer_count: u32,
    fork_count: u32,
    watchers: TotalCount,
    issues: TotalCount,
    pull_requests: TotalCount,
    releases: TotalCount,
    primary_language: Option<Language>,
    languages: GraphQLLanguages,
    license_info: Option<License>,
    default_branch_ref: Option<Branch>,
    repository_topics: TopicConnection,
}

#[derive(Deserialize)]
struct GraphQLLanguages {
    edges: Vec<LanguageEdge>,
    #[serde(rename = "pageInfo")]
    page_info: LanguagePageInfo,
}

#[derive(Deserialize)]
struct LanguagePageInfo {
    #[serde(rename = "hasNextPage")]
    has_next_page: bool,
}

impl From<GraphQLRepository> for Repository {
    fn from(repo: GraphQLRepository) -> Self {
        Self {
            node_id: repo.node_id,
            database_id: repo.database_id,
            name: repo.name,
            name_with_owner: repo.name_with_owner,
            description: repo.description,
            url: repo.url,
            homepage_url: repo.homepage_url,
            created_at: repo.created_at,
            updated_at: repo.updated_at,
            pushed_at: repo.pushed_at,
            is_private: repo.is_private,
            is_fork: repo.is_fork,
            is_archived: repo.is_archived,
            stargazer_count: repo.stargazer_count,
            fork_count: repo.fork_count,
            watcher_count: Some(repo.watchers.total_count),
            open_issue_count: Some(repo.issues.total_count),
            pull_request_count: Some(repo.pull_requests.total_count),
            release_count: Some(repo.releases.total_count),
            primary_language: repo.primary_language,
            languages: Some(
                repo.languages
                    .edges
                    .into_iter()
                    .map(|edge| crate::github::models::LanguageUsage {
                        language: edge.node,
                        bytes: edge.size,
                    })
                    .collect(),
            ),
            languages_complete: !repo.languages.page_info.has_next_page,
            license_info: repo.license_info,
            default_branch: repo.default_branch_ref.map(|branch| branch.name),
            topics: repo
                .repository_topics
                .edges
                .into_iter()
                .map(|edge| edge.node.topic.name)
                .collect(),
        }
    }
}

#[derive(Deserialize)]
struct RepositoryResponse {
    repository: Option<GraphQLRepository>,
}

pub async fn get_repository_info(
    client: &GitHubClient,
    owner: &str,
    name: &str,
) -> Result<Repository> {
    let data: RepositoryResponse = super::response::query(
        client,
        GRAPHQL_REPOSITORY_QUERY,
        json!({"owner": owner, "name": name}),
    )
    .await
    .map_err(|error| error.with_repository_context(owner, name))?;
    data.repository
        .map(Into::into)
        .ok_or_else(|| GitHubError::NotFoundError(format!("{owner}/{name}")))
}
