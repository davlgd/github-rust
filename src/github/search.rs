use crate::github::client::GitHubClient;
use crate::github::types::*;
use crate::{config::*, error::*};
use chrono::{Duration, Utc};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashSet;

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

    Some(trimmed.to_string())
}

pub use crate::github::models::SearchRepository;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphQLSearchRepository {
    #[serde(rename = "id")]
    node_id: String,
    database_id: Option<u64>,
    name: String,
    name_with_owner: String,
    description: Option<String>,
    url: String,
    stargazer_count: u32,
    fork_count: u32,
    created_at: String,
    updated_at: String,
    pushed_at: Option<String>,
    primary_language: Option<Language>,
    license_info: Option<License>,
    repository_topics: TopicConnection,
}

impl From<GraphQLSearchRepository> for SearchRepository {
    fn from(repo: GraphQLSearchRepository) -> Self {
        Self {
            node_id: repo.node_id,
            database_id: repo.database_id,
            name: repo.name,
            name_with_owner: repo.name_with_owner,
            description: repo.description,
            url: repo.url,
            stargazer_count: repo.stargazer_count,
            fork_count: repo.fork_count,
            created_at: repo.created_at,
            updated_at: repo.updated_at,
            pushed_at: repo.pushed_at,
            primary_language: repo.primary_language,
            license_info: repo.license_info,
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
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
    edges: Vec<SearchEdge>,
}

#[derive(Deserialize)]
struct SearchEdge {
    node: GraphQLSearchRepository,
}

/// Search for repositories created in the last N days with minimum stars.
pub async fn search_repositories(
    client: &GitHubClient,
    days_back: u32,
    limit: usize,
    language: Option<&str>,
    min_stars: u32,
) -> Result<Vec<SearchRepository>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let now = Utc::now();
    let days_ago = now
        .checked_sub_signed(Duration::days(i64::from(days_back)))
        .ok_or_else(|| {
            GitHubError::InvalidInput("days_back exceeds the supported date range".into())
        })?;
    let date_filter = days_ago.format("%Y-%m-%d").to_string();

    let mut query_parts = vec![
        format!("created:>{}", date_filter),
        format!("stars:>={}", min_stars),
        "is:public".to_string(),
        "sort:stars-desc".to_string(),
    ];

    if let Some(lang) = language {
        if let Some(validated_lang) = validate_language(lang) {
            query_parts.push(format!("language:\"{}\"", validated_lang));
        } else {
            return Err(GitHubError::InvalidInput(format!(
                "Invalid language parameter: '{}'. Language must contain only alphanumeric characters, spaces, hyphens, plus signs, hash, or dots.",
                lang
            )));
        }
    }

    if !client.has_token() {
        return Err(GitHubError::AuthenticationError(
            "Repository search requires a GitHub token".into(),
        ));
    }
    let query_string = query_parts.join(" ");
    tracing::debug!("GitHub search query: {}", query_string);

    let mut all_repositories = Vec::new();
    let mut after_cursor: Option<String> = None;
    let max_total = limit.min(1000);
    let mut seen_cursors = HashSet::new();

    loop {
        let data: SearchResult = super::response::query(
            client,
            GRAPHQL_SEARCH_REPOSITORIES_QUERY,
            json!({
                "queryString": query_string,
                "first": (max_total - all_repositories.len()).min(100),
                "after": after_cursor,
            }),
        )
        .await?;
        let page_is_empty = data.search.edges.is_empty();
        let page_repositories = data
            .search
            .edges
            .into_iter()
            .map(|edge| SearchRepository::from(edge.node));
        all_repositories.extend(page_repositories);
        if data.search.page_info.has_next_page && all_repositories.len() < max_total {
            let cursor = data
                .search
                .page_info
                .end_cursor
                .filter(|cursor| !cursor.is_empty())
                .ok_or_else(|| {
                    GitHubError::PaginationError("hasNextPage without endCursor".into())
                })?;
            if page_is_empty || !seen_cursors.insert(cursor.clone()) {
                return Err(GitHubError::PaginationError(
                    "Search pagination made no progress".into(),
                ));
            }
            after_cursor = Some(cursor);
        } else {
            break;
        }
    }

    all_repositories.truncate(max_total);
    Ok(all_repositories)
}
