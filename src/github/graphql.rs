use crate::github::client::GitHubClient;
use crate::github::types::*;
use crate::{config::*, error::*};
use serde::Deserialize;
use std::collections::HashMap;

pub use crate::github::models::Repository;

/// Full repository information from GitHub API.
///
/// Contains comprehensive details about a repository including metadata,
/// statistics, and related information.
#[derive(Deserialize)]
struct GraphQLRepository {
    /// Opaque global node ID, shared by the REST and GraphQL APIs.
    #[serde(rename = "id")]
    pub node_id: String,
    /// Numeric database ID, when supplied by GitHub.
    #[serde(rename = "databaseId")]
    pub database_id: Option<u64>,
    /// Repository name (without owner)
    pub name: String,
    /// Full repository name in "owner/repo" format
    #[serde(rename = "nameWithOwner")]
    pub name_with_owner: String,
    /// Repository description
    pub description: Option<String>,
    /// GitHub URL for the repository
    pub url: String,
    /// Custom homepage URL if set
    #[serde(rename = "homepageUrl")]
    pub homepage_url: Option<String>,
    /// ISO 8601 timestamp when repository was created
    #[serde(rename = "createdAt")]
    pub created_at: String,
    /// ISO 8601 timestamp of last update
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
    /// ISO 8601 timestamp of last push
    #[serde(rename = "pushedAt")]
    pub pushed_at: Option<String>,
    /// Whether the repository is private
    #[serde(rename = "isPrivate")]
    pub is_private: bool,
    /// Whether the repository is a fork
    #[serde(rename = "isFork")]
    pub is_fork: bool,
    /// Whether the repository is archived
    #[serde(rename = "isArchived")]
    pub is_archived: bool,
    /// Number of stars
    #[serde(rename = "stargazerCount")]
    pub stargazer_count: u32,
    /// Number of forks
    #[serde(rename = "forkCount")]
    pub fork_count: u32,
    /// Number of watchers
    pub watchers: TotalCount,
    /// Number of open issues
    pub issues: TotalCount,
    /// Number of pull requests
    #[serde(rename = "pullRequests")]
    pub pull_requests: TotalCount,
    /// Number of releases
    pub releases: TotalCount,
    /// Primary programming language
    #[serde(rename = "primaryLanguage")]
    pub primary_language: Option<Language>,
    /// All languages used in the repository
    pub languages: GraphQLLanguages,
    /// License information
    #[serde(rename = "licenseInfo")]
    pub license_info: Option<License>,
    /// Default branch reference
    #[serde(rename = "defaultBranchRef")]
    pub default_branch_ref: Option<Branch>,
    /// Repository topics/tags
    #[serde(rename = "repositoryTopics")]
    pub repository_topics: TopicConnection,
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
    let mut variables = HashMap::new();
    variables.insert("owner".to_string(), owner.to_string());
    variables.insert("name".to_string(), name.to_string());

    let query: GraphQLQuery<HashMap<String, String>> = GraphQLQuery {
        query: GRAPHQL_REPOSITORY_QUERY.to_string(),
        variables,
    };

    let response = client
        .post(client.graphql_url())
        .json(&query)
        .send()
        .await?;

    let data: RepositoryResponse = super::response::graphql(response)
        .await
        .map_err(|error| error.with_repository_context(owner, name))?;
    data.repository
        .map(Into::into)
        .ok_or_else(|| GitHubError::NotFoundError(format!("{owner}/{name}")))
}
