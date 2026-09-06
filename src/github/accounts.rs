//! Authenticated account and owned-repository collections.
use super::{
    pagination::{Connection, FetchOptions, Tracker},
    response,
    types::Language,
};
use crate::{GitHubClient, GitHubError, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Account {
    pub node_id: String,
    pub login: String,
    pub name: Option<String>,
    pub avatar_url: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Viewer {
    pub account: Account,
    /// All organizations visible to the token, sorted by login.
    pub organizations: Vec<Account>,
}
/// Lightweight inventory metadata, including separate counts of open issues and PRs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositorySummary {
    pub node_id: String,
    pub name: String,
    pub name_with_owner: String,
    pub description: Option<String>,
    pub url: String,
    pub is_private: bool,
    pub is_fork: bool,
    pub is_archived: bool,
    pub stargazer_count: u32,
    pub fork_count: u32,
    pub pushed_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
    pub primary_language: Option<Language>,
    pub open_issue_count: u32,
    pub open_pull_request_count: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnedRepositories {
    pub owner: Account,
    pub repositories: Vec<RepositorySummary>,
}
/// A validated page. Earlier pages remain provisional until the whole call succeeds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryPage {
    pub owner: Account,
    pub repositories: Vec<RepositorySummary>,
    pub total_count: usize,
    pub has_next_page: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountDto {
    id: String,
    login: String,
    name: Option<String>,
    avatar_url: String,
}
impl From<AccountDto> for Account {
    fn from(v: AccountDto) -> Self {
        Self {
            node_id: v.id,
            login: v.login,
            name: v.name,
            avatar_url: v.avatar_url,
        }
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepositoryDto {
    id: String,
    name: String,
    name_with_owner: String,
    description: Option<String>,
    url: String,
    is_private: bool,
    is_fork: bool,
    is_archived: bool,
    stargazer_count: u32,
    fork_count: u32,
    pushed_at: Option<DateTime<Utc>>,
    updated_at: DateTime<Utc>,
    primary_language: Option<Language>,
    issues: Count,
    pull_requests: Count,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Count {
    pub total_count: u32,
}
impl From<RepositoryDto> for RepositorySummary {
    fn from(v: RepositoryDto) -> Self {
        Self {
            node_id: v.id,
            name: v.name,
            name_with_owner: v.name_with_owner,
            description: v.description,
            url: v.url,
            is_private: v.is_private,
            is_fork: v.is_fork,
            is_archived: v.is_archived,
            stargazer_count: v.stargazer_count,
            fork_count: v.fork_count,
            pushed_at: v.pushed_at,
            updated_at: v.updated_at,
            primary_language: v.primary_language,
            open_issue_count: v.issues.total_count,
            open_pull_request_count: v.pull_requests.total_count,
        }
    }
}
pub(crate) fn require_token(client: &GitHubClient) -> Result<()> {
    if !client.has_token() {
        return Err(GitHubError::AuthenticationError(
            "This operation requires a token".into(),
        ));
    }
    Ok(())
}
pub(crate) fn validate_owner(owner: &str) -> Result<()> {
    // Underscores also accommodate Enterprise Managed User logins.
    if owner.is_empty()
        || owner.len() > 100
        || !owner
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err(GitHubError::InvalidInput("Invalid owner login".into()));
    }
    Ok(())
}
pub(crate) async fn query<T: serde::de::DeserializeOwned>(
    client: &GitHubClient,
    query: &str,
    variables: serde_json::Value,
) -> Result<T> {
    response::graphql(
        client
            .post(client.graphql_url())
            .json(&json!({"query": query, "variables": variables}))
            .send()
            .await?,
    )
    .await
}
pub(crate) async fn viewer(client: &GitHubClient, options: FetchOptions) -> Result<Viewer> {
    require_token(client)?;
    #[derive(Deserialize)]
    struct Data {
        viewer: ViewerDto,
    }
    #[derive(Deserialize)]
    struct ViewerDto {
        #[serde(flatten)]
        account: AccountDto,
        organizations: Connection<AccountDto>,
    }
    let mut tracker = Tracker::default();
    let mut result: Option<Viewer> = None;
    loop {
        let data: Data = query(
            client,
            include_str!("queries/viewer.graphql"),
            json!({"first": options.page_size, "cursor": tracker.cursor}),
        )
        .await?;
        let account: Account = data.viewer.account.into();
        if account.node_id.is_empty()
            || result.as_ref().is_some_and(|v| {
                v.account.node_id != account.node_id || v.account.login != account.login
            })
        {
            return Err(GitHubError::PaginationError(
                "Viewer changed during pagination".into(),
            ));
        }
        tracker.accept(&data.viewer.organizations, options, |a| &a.id)?;
        let result = result.get_or_insert_with(|| Viewer {
            account,
            organizations: vec![],
        });
        result
            .organizations
            .extend(data.viewer.organizations.nodes.into_iter().map(Into::into));
        if tracker.cursor.is_none() {
            result.organizations.sort_by(|a, b| a.login.cmp(&b.login));
            break;
        }
    }
    Ok(result.expect("at least one page"))
}
pub(crate) async fn repositories<F>(
    client: &GitHubClient,
    login: &str,
    options: FetchOptions,
    mut progress: Option<F>,
) -> Result<OwnedRepositories>
where
    F: AsyncFnMut(&RepositoryPage) -> Result<()>,
{
    validate_owner(login)?;
    require_token(client)?;
    #[derive(Deserialize)]
    struct Data {
        #[serde(rename = "repositoryOwner")]
        owner: Option<OwnerDto>,
    }
    #[derive(Deserialize)]
    struct OwnerDto {
        #[serde(flatten)]
        account: AccountDto,
        repositories: Connection<RepositoryDto>,
    }
    let mut tracker = Tracker::default();
    let mut result: Option<OwnedRepositories> = None;
    loop {
        let data: Data = query(
            client,
            include_str!("queries/repositories.graphql"),
            json!({"login": login, "first": options.page_size, "cursor": tracker.cursor}),
        )
        .await?;
        let owner = data
            .owner
            .ok_or_else(|| GitHubError::NotFoundError(login.into()))?;
        let account: Account = owner.account.into();
        if account.node_id.is_empty()
            || !account.login.eq_ignore_ascii_case(login)
            || result.as_ref().is_some_and(|r| {
                r.owner.node_id != account.node_id || r.owner.login != account.login
            })
            || owner.repositories.nodes.iter().any(|r| {
                !r.name_with_owner
                    .eq_ignore_ascii_case(&format!("{}/{}", account.login, r.name))
            })
        {
            return Err(GitHubError::PaginationError(
                "Repository owner changed during pagination".into(),
            ));
        }
        tracker.accept(&owner.repositories, options, |r| &r.id)?;
        let page = RepositoryPage {
            owner: account.clone(),
            total_count: owner.repositories.total_count,
            has_next_page: owner.repositories.page_info.has_next_page,
            repositories: owner
                .repositories
                .nodes
                .into_iter()
                .map(Into::into)
                .collect(),
        };
        let result = result.get_or_insert_with(|| OwnedRepositories {
            owner: account,
            repositories: vec![],
        });
        if let Some(callback) = &mut progress {
            callback(&page).await?;
        }
        result.repositories.extend(page.repositories);
        if tracker.cursor.is_none() {
            result
                .repositories
                .sort_by(|a, b| a.name_with_owner.cmp(&b.name_with_owner));
            break;
        }
    }
    Ok(result.expect("at least one page"))
}
