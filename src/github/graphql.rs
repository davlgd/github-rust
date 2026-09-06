use crate::github::client::GitHubClient;
use crate::github::types::*;
use crate::{config::*, error::*};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Full repository information from GitHub API.
///
/// Contains comprehensive details about a repository including metadata,
/// statistics, and related information.
#[derive(Deserialize, Serialize)]
pub struct Repository {
    /// GitHub's internal ID for the repository
    pub id: String,
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
    pub languages: LanguageConnection,
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

impl Repository {
    /// Returns the primary language name, or None if not set.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use github_rust::GitHubService;
    /// # async fn example() -> github_rust::Result<()> {
    /// let service = GitHubService::new()?;
    /// let repo = service.get_repository_info("rust-lang", "rust").await?;
    ///
    /// if let Some(lang) = repo.language() {
    ///     println!("Primary language: {}", lang);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn language(&self) -> Option<&str> {
        self.primary_language.as_ref().map(|l| l.name.as_str())
    }

    /// Returns the license name, or None if not set.
    #[must_use]
    pub fn license(&self) -> Option<&str> {
        self.license_info.as_ref().map(|l| l.name.as_str())
    }

    /// Returns the SPDX license identifier, or None if not available.
    #[must_use]
    pub fn license_spdx(&self) -> Option<&str> {
        self.license_info
            .as_ref()
            .and_then(|l| l.spdx_id.as_deref())
    }

    /// Returns the default branch name, or None if not set.
    #[must_use]
    pub fn default_branch(&self) -> Option<&str> {
        self.default_branch_ref.as_ref().map(|b| b.name.as_str())
    }

    /// Returns a list of topic names.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use github_rust::GitHubService;
    /// # async fn example() -> github_rust::Result<()> {
    /// let service = GitHubService::new()?;
    /// let repo = service.get_repository_info("rust-lang", "rust").await?;
    ///
    /// for topic in repo.topics() {
    ///     println!("Topic: {}", topic);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn topics(&self) -> Vec<&str> {
        self.repository_topics
            .edges
            .iter()
            .map(|e| e.node.topic.name.as_str())
            .collect()
    }

    /// Returns the owner part of name_with_owner.
    #[must_use]
    pub fn owner(&self) -> &str {
        self.name_with_owner
            .split('/')
            .next()
            .unwrap_or(&self.name_with_owner)
    }

    /// Returns the number of open issues.
    #[must_use]
    pub fn open_issues(&self) -> u32 {
        self.issues.total_count
    }

    /// Returns the number of watchers.
    #[must_use]
    pub fn watcher_count(&self) -> u32 {
        self.watchers.total_count
    }
}

#[derive(Deserialize)]
struct RepositoryResponse {
    repository: Option<Repository>,
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

    let status = response.status();
    if !status.is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return match status.as_u16() {
            401 => Err(GitHubError::AuthenticationError(
                "Invalid or missing GitHub token".to_string(),
            )),
            403 => {
                // Parse HTTP 403 more intelligently
                let error_lower = error_text.to_lowercase();
                if error_lower.contains("rate limit")
                    || error_lower.contains("api rate limit exceeded")
                {
                    Err(GitHubError::RateLimitError(
                        "GraphQL API rate limit exceeded".to_string(),
                    ))
                } else if error_lower.contains("repository access blocked")
                    || error_lower.contains("access blocked")
                    || error_lower.contains("blocked")
                {
                    Err(GitHubError::AccessBlockedError(format!(
                        "{}/{}",
                        owner, name
                    )))
                } else {
                    // Generic access denied (permissions, private repo, etc.)
                    Err(GitHubError::AuthenticationError(format!(
                        "Access denied to {}/{}: {}",
                        owner, name, error_text
                    )))
                }
            }
            404 => Err(GitHubError::NotFoundError(format!("{}/{}", owner, name))),
            451 => Err(GitHubError::DmcaBlockedError(format!("{}/{}", owner, name))),
            _ => Err(GitHubError::ApiError {
                status: status.as_u16(),
                message: error_text,
            }),
        };
    }

    let graphql_response: GraphQLResponse<RepositoryResponse> = response.json().await?;

    if let Some(errors) = graphql_response.errors {
        let error_message = errors
            .into_iter()
            .map(|e| e.message)
            .collect::<Vec<_>>()
            .join(", ");
        return Err(GitHubError::ApiError {
            status: 200,
            message: error_message,
        });
    }

    match graphql_response.data {
        Some(data) => match data.repository {
            Some(repo) => Ok(repo),
            None => Err(GitHubError::NotFoundError(format!("{}/{}", owner, name))),
        },
        None => Err(GitHubError::ParseError(
            "No data in GraphQL response".to_string(),
        )),
    }
}
