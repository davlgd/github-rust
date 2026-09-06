//! Open issues and pull requests, with bounded multi-repository traversal.
use super::{
    accounts::{Count, require_token, validate_owner},
    pagination::{Connection, FetchOptions, Tracker},
    response::query,
};
use crate::{GitHubClient, GitHubError, Result};
use chrono::{DateTime, Utc};
use futures_util::{
    Stream, StreamExt, TryStreamExt,
    stream::{self, FuturesUnordered},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::HashSet, sync::Arc};

/// Explicit scope for collection calls. All scopes are validated before sending requests.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepositoryCoordinates {
    pub owner: String,
    pub name: String,
}
impl RepositoryCoordinates {
    pub fn new(owner: impl Into<String>, name: impl Into<String>) -> Result<Self> {
        let value = Self {
            owner: owner.into(),
            name: name.into(),
        };
        value.validate()?;
        Ok(value)
    }
    fn validate(&self) -> Result<()> {
        validate_owner(&self.owner)?;
        if self.name.is_empty()
            || self.name.len() > 100
            || matches!(self.name.as_str(), "." | "..")
            || !self
                .name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
        {
            return Err(GitHubError::InvalidInput("Invalid repository name".into()));
        }
        Ok(())
    }
}
impl super::RepositorySummary {
    pub fn coordinates(&self) -> Result<RepositoryCoordinates> {
        let (owner, name) =
            crate::parse_repository(&self.name_with_owner).map_err(GitHubError::InvalidInput)?;
        RepositoryCoordinates::new(owner, name)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepositoryReference {
    pub node_id: String,
    pub name: String,
    pub name_with_owner: String,
    pub is_archived: bool,
}
/// An issue author may be a user, organization or bot; deleted authors are `None`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Actor {
    pub login: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Label {
    pub name: String,
    pub color: String,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewDecision {
    Approved,
    ChangesRequested,
    ReviewRequired,
}

/// A validated repository page; pages across repositories arrive in completion order.
/// Only successful completion of the entire traversal certifies all scopes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkItemPage<T> {
    pub repository: RepositoryReference,
    pub items: Vec<T>,
    pub total_count: usize,
    /// Whether this repository has more pages; other scopes may still be incomplete.
    pub has_next_page: bool,
}

/// An open issue with complete labels and assignees.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    pub node_id: String,
    pub number: u64,
    pub title: String,
    pub url: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub author: Option<Actor>,
    pub repository: RepositoryReference,
    pub labels: Vec<Label>,
    pub assignees: Vec<Actor>,
    pub comment_count: u32,
}

/// An open pull request with complete labels and assignees.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequest {
    pub node_id: String,
    pub number: u64,
    pub title: String,
    pub url: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub author: Option<Actor>,
    pub repository: RepositoryReference,
    pub labels: Vec<Label>,
    pub assignees: Vec<Actor>,
    pub comment_count: u32,
    pub is_draft: bool,
    pub review_decision: Option<ReviewDecision>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NodeDto {
    id: String,
    number: u64,
    title: String,
    url: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    author: Option<Actor>,
    labels: Metadata<Label>,
    assignees: Metadata<Actor>,
    comments: Count,
    is_draft: Option<bool>,
    review_decision: Option<ReviewDecision>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Metadata<T> {
    nodes: Vec<T>,
    total_count: usize,
}
impl<T> Metadata<T> {
    fn complete(self) -> Result<Vec<T>> {
        if self.nodes.len() != self.total_count {
            return Err(GitHubError::PaginationError(
                "Label or assignee count does not match returned nodes; refusing incomplete or inconsistent metadata".into(),
            ));
        }
        Ok(self.nodes)
    }
}
pub(crate) trait Item: Sized + Send {
    const ISSUES: bool;
    fn from_wire(node: NodeDto, repository: &RepositoryReference) -> Result<Self>;
    fn id(&self) -> &str;
    fn updated(&self) -> DateTime<Utc>;
}

impl Item for Issue {
    const ISSUES: bool = true;
    fn from_wire(n: NodeDto, repository: &RepositoryReference) -> Result<Self> {
        Ok(Self {
            node_id: n.id,
            number: n.number,
            title: n.title,
            url: n.url,
            created_at: n.created_at,
            updated_at: n.updated_at,
            author: n.author,
            repository: repository.clone(),
            labels: n.labels.complete()?,
            assignees: n.assignees.complete()?,
            comment_count: n.comments.total_count,
        })
    }
    fn id(&self) -> &str {
        &self.node_id
    }
    fn updated(&self) -> DateTime<Utc> {
        self.updated_at
    }
}

impl Item for PullRequest {
    const ISSUES: bool = false;
    fn from_wire(n: NodeDto, repository: &RepositoryReference) -> Result<Self> {
        Ok(Self {
            node_id: n.id,
            number: n.number,
            title: n.title,
            url: n.url,
            created_at: n.created_at,
            updated_at: n.updated_at,
            author: n.author,
            repository: repository.clone(),
            labels: n.labels.complete()?,
            assignees: n.assignees.complete()?,
            comment_count: n.comments.total_count,
            is_draft: n
                .is_draft
                .ok_or_else(|| GitHubError::ParseError("Missing PR draft status".into()))?,
            review_decision: n.review_decision,
        })
    }
    fn id(&self) -> &str {
        &self.node_id
    }
    fn updated(&self) -> DateTime<Utc> {
        self.updated_at
    }
}

struct Traversal {
    scope: RepositoryCoordinates,
    identity: Option<RepositoryReference>,
    tracker: Tracker,
}
async fn next_page<T: Item>(
    client: Arc<GitHubClient>,
    options: FetchOptions,
    mut traversal: Traversal,
) -> Result<(Traversal, WorkItemPage<T>)> {
    #[derive(Deserialize)]
    struct Data {
        repository: Option<RepositoryDto>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct RepositoryDto {
        id: String,
        name: String,
        name_with_owner: String,
        is_archived: bool,
        issues: Option<Connection<NodeDto>>,
        pull_requests: Option<Connection<NodeDto>>,
    }
    let scope = &traversal.scope;
    let data: Data = query(
        &client,
        include_str!("queries/work_items.graphql"),
        json!({
            "owner": scope.owner, "name": scope.name, "cursor": traversal.tracker.cursor,
            "first": options.page_size, "issues": T::ISSUES
        }),
    )
    .await
    .map_err(|e| e.with_repository_context(&scope.owner, &scope.name))?;
    let repo = data
        .repository
        .ok_or_else(|| GitHubError::NotFoundError(format!("{}/{}", scope.owner, scope.name)))?;
    let identity = RepositoryReference {
        node_id: repo.id,
        name: repo.name,
        name_with_owner: repo.name_with_owner,
        is_archived: repo.is_archived,
    };
    if identity.node_id.is_empty()
        || !identity.name.eq_ignore_ascii_case(&scope.name)
        || !identity
            .name_with_owner
            .eq_ignore_ascii_case(&format!("{}/{}", scope.owner, scope.name))
        || traversal
            .identity
            .as_ref()
            .is_some_and(|previous| previous != &identity)
    {
        return Err(GitHubError::PaginationError(
            "Repository identity or archive status changed during pagination".into(),
        ));
    }
    let connection = if T::ISSUES {
        repo.issues
    } else {
        repo.pull_requests
    }
    .ok_or_else(|| GitHubError::ParseError("Missing work-item connection".into()))?;
    traversal.tracker.accept(&connection, options, |n| &n.id)?;
    let items = connection
        .nodes
        .into_iter()
        .map(|n| T::from_wire(n, &identity))
        .collect::<Result<Vec<_>>>()?;
    traversal.identity = Some(identity.clone());
    Ok((
        traversal,
        WorkItemPage {
            repository: identity,
            items,
            total_count: connection.total_count,
            has_next_page: connection.page_info.has_next_page,
        },
    ))
}

pub(crate) fn pages<T: Item>(
    client: GitHubClient,
    scopes: Vec<RepositoryCoordinates>,
    options: FetchOptions,
) -> impl Stream<Item = Result<WorkItemPage<T>>> + Send {
    stream::once(async move {
        // Share the owned client across in-flight requests without copying its token.
        let client = Arc::new(client);
        let mut names = HashSet::new();
        for scope in &scopes {
            scope.validate()?;
            if !names.insert(format!("{}/{}", scope.owner, scope.name).to_ascii_lowercase()) {
                return Err(GitHubError::InvalidInput(
                    "Duplicate repository scope".into(),
                ));
            }
        }
        if !scopes.is_empty() {
            require_token(&client)?;
        }
        let mut scopes = scopes.into_iter().map(|scope| Traversal {
            scope,
            identity: None,
            tracker: Tracker::default(),
        });
        let pending = FuturesUnordered::new();
        for traversal in scopes.by_ref().take(options.max_concurrent_repositories) {
            pending.push(next_page::<T>(client.clone(), options, traversal));
        }
        Ok(stream::try_unfold(
            (scopes, pending, HashSet::new()),
            move |(mut scopes, mut pending, mut ids)| {
                let client = client.clone();
                async move {
                    let Some(page) = pending.next().await else {
                        return Ok(None);
                    };
                    let (traversal, page) = page?;
                    for item in &page.items {
                        if !ids.insert(item.id().to_owned()) {
                            return Err(GitHubError::PaginationError(
                                "Work item appeared in multiple repository scopes".into(),
                            ));
                        }
                    }
                    // These futures stay unpolled until the consumer requests another page.
                    // Dropping the stream also drops every pending request.
                    if traversal.tracker.cursor.is_some() {
                        pending.push(next_page::<T>(client, options, traversal));
                    } else if let Some(next) = scopes.next() {
                        pending.push(next_page::<T>(client, options, next));
                    }
                    Ok(Some((page, (scopes, pending, ids))))
                }
            },
        ))
    })
    .try_flatten()
}

pub(crate) async fn collect<T, F>(
    client: &GitHubClient,
    scopes: &[RepositoryCoordinates],
    options: FetchOptions,
    mut progress: Option<F>,
) -> Result<Vec<T>>
where
    T: Item,
    F: AsyncFnMut(&WorkItemPage<T>) -> Result<()>,
{
    let pages = pages::<T>(client.clone(), scopes.to_vec(), options);
    futures_util::pin_mut!(pages);
    let mut result = vec![];
    while let Some(page) = pages.try_next().await? {
        if let Some(callback) = &mut progress {
            callback(&page).await?;
        }
        result.extend(page.items);
    }
    result.sort_by(|a: &T, b| {
        b.updated()
            .cmp(&a.updated())
            .then_with(|| a.id().cmp(b.id()))
    });
    Ok(result)
}
