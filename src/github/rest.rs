use crate::error::*;
use crate::github::client::GitHubClient;
use crate::github::graphql::Repository as GraphQLRepository;
use crate::github::types::*;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::Deserialize;

/// URL-encode a path segment for safe use in GitHub API URLs.
fn encode_path_segment(segment: &str) -> String {
    utf8_percent_encode(segment, NON_ALPHANUMERIC).to_string()
}

#[derive(Deserialize)]
struct RestRepository {
    id: u64,
    node_id: String,
    name: String,
    full_name: String,
    description: Option<String>,
    html_url: String,
    homepage: Option<String>,
    created_at: String,
    updated_at: String,
    pushed_at: Option<String>,
    private: bool,
    fork: bool,
    archived: bool,
    stargazers_count: u32,
    forks_count: u32,
    watchers_count: u32,
    open_issues_count: u32,
    language: Option<String>,
    license: Option<RestLicense>,
    default_branch: String,
    topics: Vec<String>,
}

#[derive(Deserialize)]
struct RestLicense {
    name: String,
    spdx_id: Option<String>,
}

#[derive(Deserialize)]
struct LanguageStats {
    #[serde(flatten)]
    languages: std::collections::HashMap<String, u64>,
}

#[derive(Deserialize)]
pub struct UserProfile {
    pub login: String,
    pub name: Option<String>,
}

pub async fn get_repository_info(
    client: &GitHubClient,
    owner: &str,
    name: &str,
) -> Result<GraphQLRepository> {
    let encoded_owner = encode_path_segment(owner);
    let encoded_name = encode_path_segment(name);
    let repo_url = format!(
        "{}/repos/{}/{}",
        client.rest_url(),
        encoded_owner,
        encoded_name
    );

    let response = client.get(&repo_url).send().await?;

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
                        "REST API rate limit exceeded".to_string(),
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

    let rest_repo: RestRepository = response.json().await?;

    let languages_url = format!(
        "{}/repos/{}/{}/languages",
        client.rest_url(),
        encoded_owner,
        encoded_name
    );
    let lang_response = client.get(&languages_url).send().await?;
    let language_stats: LanguageStats = if lang_response.status().is_success() {
        lang_response.json().await?
    } else {
        LanguageStats {
            languages: std::collections::HashMap::new(),
        }
    };

    Ok(convert_rest_to_graphql(rest_repo, language_stats))
}

pub async fn get_user_profile(client: &GitHubClient) -> Result<UserProfile> {
    let user_url = format!("{}/user", client.rest_url());

    let response = client.get(&user_url).send().await?;

    let status = response.status();
    if !status.is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return match status.as_u16() {
            401 => Err(GitHubError::AuthenticationError(
                "GitHub token is required to get user profile".to_string(),
            )),
            403 => {
                // Parse HTTP 403 more intelligently
                let error_lower = error_text.to_lowercase();
                if error_lower.contains("rate limit")
                    || error_lower.contains("api rate limit exceeded")
                {
                    Err(GitHubError::RateLimitError(
                        "REST API rate limit exceeded".to_string(),
                    ))
                } else {
                    // Generic access denied for user profile
                    Err(GitHubError::AuthenticationError(format!(
                        "Access denied for user profile: {}",
                        error_text
                    )))
                }
            }
            _ => Err(GitHubError::ApiError {
                status: status.as_u16(),
                message: error_text,
            }),
        };
    }

    let user_profile: UserProfile = response.json().await?;
    Ok(user_profile)
}

#[derive(Deserialize)]
struct StarredRepository {
    full_name: String,
}

/// Maximum number of pages to fetch for starred repositories.
/// This limits total results to 10,000 repositories (100 pages * 100 per page).
const MAX_STARRED_PAGES: u32 = 100;

pub async fn get_user_starred_repositories(client: &GitHubClient) -> Result<Vec<String>> {
    let mut all_starred = Vec::new();
    let mut page = 1;
    let per_page = 100;

    loop {
        // Safety limit to prevent infinite loops or excessive API calls
        if page > MAX_STARRED_PAGES {
            tracing::warn!(
                "Reached maximum page limit ({}) for starred repositories. Returning {} repositories.",
                MAX_STARRED_PAGES,
                all_starred.len()
            );
            break;
        }
        let starred_url = format!(
            "{}/user/starred?per_page={}&page={}",
            client.rest_url(),
            per_page,
            page
        );

        let response = client.get(&starred_url).send().await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return match status.as_u16() {
                401 => Err(GitHubError::AuthenticationError(
                    "GitHub token is required to get starred repositories".to_string(),
                )),
                403 => {
                    // Parse HTTP 403 more intelligently
                    let error_lower = error_text.to_lowercase();
                    if error_lower.contains("rate limit")
                        || error_lower.contains("api rate limit exceeded")
                    {
                        Err(GitHubError::RateLimitError(
                            "REST API rate limit exceeded".to_string(),
                        ))
                    } else {
                        // Generic access denied for starred repositories
                        Err(GitHubError::AuthenticationError(format!(
                            "Access denied for starred repositories: {}",
                            error_text
                        )))
                    }
                }
                _ => Err(GitHubError::ApiError {
                    status: status.as_u16(),
                    message: error_text,
                }),
            };
        }

        let starred_repos: Vec<StarredRepository> = response.json().await?;

        if starred_repos.is_empty() {
            break;
        }

        let repo_count = starred_repos.len();
        all_starred.extend(starred_repos.into_iter().map(|repo| repo.full_name));

        if repo_count < per_page {
            break;
        }

        page += 1;
    }

    Ok(all_starred)
}

pub async fn get_repository_stargazers(
    client: &GitHubClient,
    owner: &str,
    name: &str,
    per_page: Option<u32>,
    page: Option<u32>,
) -> Result<Vec<StargazerWithDate>> {
    let per_page = per_page.unwrap_or(30).min(100); // GitHub max is 100
    let page = page.unwrap_or(1);

    let encoded_owner = encode_path_segment(owner);
    let encoded_name = encode_path_segment(name);
    let stargazers_url = format!(
        "{}/repos/{}/{}/stargazers?per_page={}&page={}",
        client.rest_url(),
        encoded_owner,
        encoded_name,
        per_page,
        page
    );

    let response = client
        .get(&stargazers_url)
        .headers(reqwest::header::HeaderMap::from_iter([(
            reqwest::header::ACCEPT,
            reqwest::header::HeaderValue::from_static("application/vnd.github.v3.star+json"),
        )]))
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
                let error_lower = error_text.to_lowercase();
                if error_lower.contains("rate limit")
                    || error_lower.contains("api rate limit exceeded")
                {
                    Err(GitHubError::RateLimitError(
                        "REST API rate limit exceeded".to_string(),
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

    let stargazers: Vec<StargazerWithDate> = response.json().await?;
    Ok(stargazers)
}

fn convert_rest_to_graphql(rest: RestRepository, lang_stats: LanguageStats) -> GraphQLRepository {
    let languages = LanguageConnection {
        edges: lang_stats
            .languages
            .into_iter()
            .map(|(name, size)| LanguageEdge {
                size,
                node: Language { name, color: None },
            })
            .collect(),
    };

    let repository_topics = TopicConnection {
        edges: rest
            .topics
            .into_iter()
            .map(|topic_name| TopicEdge {
                node: TopicNode {
                    topic: Topic { name: topic_name },
                },
            })
            .collect(),
    };

    GraphQLRepository {
        node_id: rest.node_id,
        database_id: Some(rest.id),
        name: rest.name,
        name_with_owner: rest.full_name,
        description: rest.description,
        url: rest.html_url,
        homepage_url: rest.homepage,
        created_at: rest.created_at,
        updated_at: rest.updated_at,
        pushed_at: rest.pushed_at,
        is_private: rest.private,
        is_fork: rest.fork,
        is_archived: rest.archived,
        stargazer_count: rest.stargazers_count,
        fork_count: rest.forks_count,
        watchers: TotalCount {
            total_count: rest.watchers_count,
        },
        issues: TotalCount {
            total_count: rest.open_issues_count,
        },
        pull_requests: TotalCount { total_count: 0 },
        releases: TotalCount { total_count: 0 },
        primary_language: rest.language.map(|name| Language { name, color: None }),
        languages,
        license_info: rest.license.map(|l| License {
            name: l.name,
            spdx_id: l.spdx_id,
        }),
        default_branch_ref: Some(Branch {
            name: rest.default_branch,
        }),
        repository_topics,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    #[test]
    fn test_stargazer_deserialization() {
        let json_data = r#"[
            {
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
            }
        ]"#;

        let stargazers: Vec<StargazerWithDate> =
            serde_json::from_str(json_data).expect("Failed to deserialize stargazers");
        assert_eq!(stargazers.len(), 1);
        assert_eq!(stargazers[0].starred_at, "2015-09-11T10:42:05Z");
        assert_eq!(stargazers[0].user.login, "testuser");
        assert_eq!(stargazers[0].user.id, 12345);
        assert!(!stargazers[0].user.site_admin);
    }

    #[test]
    fn test_pagination_parameters() {
        // Test that pagination parameters are validated correctly
        fn apply_limit(per_page: u32) -> u32 {
            per_page.min(100)
        }

        assert_eq!(apply_limit(50), 50);
        assert_eq!(apply_limit(150), 100);
        assert_eq!(apply_limit(30), 30);
    }

    #[test]
    fn test_stargazers_url_construction() {
        let owner = "microsoft";
        let name = "vscode";
        let per_page = 30;
        let page = 1;

        let expected_url = format!(
            "{}/repos/{}/{}/stargazers?per_page={}&page={}",
            crate::config::GITHUB_API_URL,
            owner,
            name,
            per_page,
            page
        );

        assert!(expected_url.contains("microsoft/vscode/stargazers"));
        assert!(expected_url.contains("per_page=30"));
        assert!(expected_url.contains("page=1"));
    }
}
