use crate::github::client::GitHubClient;
use crate::github::types::*;
use crate::{config::*, error::*};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Validates and sanitizes a language parameter for GitHub search.
/// Returns None if the language is invalid, Some(sanitized) otherwise.
fn validate_language(language: &str) -> Option<String> {
    let trimmed = language.trim();

    // Empty language is invalid
    if trimmed.is_empty() {
        return None;
    }

    // Language should only contain alphanumeric, spaces, hyphens, plus, hash, and dots
    // Examples: "C++", "C#", "F#", "Objective-C", "Visual Basic .NET"
    let is_valid = trimmed
        .chars()
        .all(|c| c.is_alphanumeric() || c == ' ' || c == '-' || c == '+' || c == '#' || c == '.');

    if !is_valid {
        return None;
    }

    // Reject if it looks like a search operator injection attempt
    let lower = trimmed.to_lowercase();
    let suspicious_patterns = [
        "repo:",
        "user:",
        "org:",
        "in:",
        "size:",
        "fork:",
        "stars:",
        "pushed:",
        "created:",
        "updated:",
        "language:",
        "topic:",
        "license:",
        "is:",
        "has:",
        "good-first-issues:",
        "help-wanted-issues:",
        "archived:",
        "mirror:",
        "template:",
        "sort:",
        " or ",
        " and ",
        " not ",
    ];

    for pattern in suspicious_patterns {
        if lower.contains(pattern) {
            return None;
        }
    }

    Some(trimmed.to_string())
}

/// Repository data from GitHub search API.
///
/// A lighter-weight repository struct returned by search operations,
/// containing the most commonly needed fields.
#[derive(Deserialize, Serialize, Clone, Default, Debug)]
pub struct SearchRepository {
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
    /// Number of stars
    #[serde(rename = "stargazerCount")]
    pub stargazer_count: u32,
    /// Number of forks
    #[serde(rename = "forkCount")]
    pub fork_count: u32,
    /// ISO 8601 timestamp when repository was created
    #[serde(rename = "createdAt")]
    pub created_at: String,
    /// ISO 8601 timestamp of last update
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
    /// ISO 8601 timestamp of last push
    #[serde(rename = "pushedAt")]
    pub pushed_at: Option<String>,
    /// Primary programming language
    #[serde(rename = "primaryLanguage")]
    pub primary_language: Option<Language>,
    /// License information
    #[serde(rename = "licenseInfo")]
    pub license_info: Option<License>,
    /// Repository topics/tags
    #[serde(rename = "repositoryTopics")]
    pub repository_topics: TopicConnection,
}

impl SearchRepository {
    /// Returns the primary language name, or None if not set.
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

    /// Returns a list of topic names.
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
}

#[derive(Deserialize)]
struct SearchResult {
    search: SearchConnection,
}

#[derive(Deserialize)]
struct PageInfo {
    #[serde(rename = "hasNextPage")]
    has_next_page: bool,
    #[serde(rename = "endCursor")]
    end_cursor: Option<String>,
}

#[derive(Deserialize)]
struct SearchConnection {
    #[serde(rename = "repositoryCount")]
    #[allow(dead_code)]
    repository_count: u32,
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
    edges: Vec<SearchEdge>,
}

#[derive(Deserialize)]
struct SearchEdge {
    node: SearchRepository,
}

/// Search for repositories created in the last N days with minimum stars.
pub async fn search_repositories(
    client: &GitHubClient,
    days_back: u32,
    limit: usize,
    language: Option<&str>,
    min_stars: u32,
) -> Result<Vec<SearchRepository>> {
    let now = Utc::now();
    let days_ago = now - Duration::days(days_back as i64);
    let date_filter = days_ago.format("%Y-%m-%d").to_string();

    let mut query_parts = vec![
        format!("created:>{}", date_filter),
        format!("stars:>={}", min_stars),
        "is:public".to_string(),
        "sort:stars-desc".to_string(),
    ];

    if let Some(lang) = language {
        if let Some(validated_lang) = validate_language(lang) {
            query_parts.push(format!("language:{}", validated_lang));
        } else {
            return Err(GitHubError::InvalidInput(format!(
                "Invalid language parameter: '{}'. Language must contain only alphanumeric characters, spaces, hyphens, plus signs, hash, or dots.",
                lang
            )));
        }
    }

    let query_string = query_parts.join(" ");
    tracing::debug!("GitHub search query: {}", query_string);

    let mut all_repositories = Vec::new();
    let mut after_cursor: Option<String> = None;
    let max_total = limit.min(1000);

    loop {
        let mut variables = HashMap::new();
        variables.insert(
            "queryString".to_string(),
            serde_json::Value::String(query_string.clone()),
        );
        variables.insert(
            "first".to_string(),
            serde_json::Value::Number(serde_json::Number::from(100)),
        );

        if let Some(cursor) = &after_cursor {
            variables.insert(
                "after".to_string(),
                serde_json::Value::String(cursor.clone()),
            );
        } else {
            variables.insert("after".to_string(), serde_json::Value::Null);
        }

        let graphql_query: GraphQLQuery<HashMap<String, serde_json::Value>> = GraphQLQuery {
            query: GRAPHQL_SEARCH_REPOSITORIES_QUERY.to_string(),
            variables,
        };

        let response = client
            .post(client.graphql_url())
            .json(&graphql_query)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return match status.as_u16() {
                401 => Err(GitHubError::AuthenticationError(
                    "Invalid or missing GitHub token".to_string(),
                )),
                403 => Err(GitHubError::RateLimitError(
                    "GraphQL API rate limit exceeded".to_string(),
                )),
                451 => Err(GitHubError::DmcaBlockedError(
                    "Search blocked for legal reasons".to_string(),
                )),
                _ => Err(GitHubError::ApiError {
                    status: status.as_u16(),
                    message: error_text,
                }),
            };
        }

        let graphql_response: GraphQLResponse<SearchResult> = response.json().await?;

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
            Some(data) => {
                let page_repositories: Vec<SearchRepository> = data
                    .search
                    .edges
                    .into_iter()
                    .map(|edge| edge.node)
                    .collect();

                all_repositories.extend(page_repositories);

                if data.search.page_info.has_next_page && all_repositories.len() < max_total {
                    after_cursor = data.search.page_info.end_cursor;
                } else {
                    break;
                }
            }
            None => {
                return Err(GitHubError::ParseError(
                    "No data in GraphQL response".to_string(),
                ));
            }
        }
    }

    all_repositories.truncate(max_total);
    Ok(all_repositories)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_repository_default() {
        let repo = SearchRepository::default();
        assert_eq!(repo.stargazer_count, 0);
        assert_eq!(repo.fork_count, 0);
        assert!(repo.description.is_none());
    }
}
