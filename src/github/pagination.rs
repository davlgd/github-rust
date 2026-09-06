//! Strict connection traversal shared by account and work-item queries.
use crate::{GitHubError, Result};
use serde::Deserialize;
use std::collections::HashSet;

/// Limits for account, owned-repository and open-work-item collection methods.
/// Existing search and REST pagination retain their own limits.
#[derive(Debug, Clone, Copy)]
pub struct FetchOptions {
    /// Nodes requested per page, from 1 to 100. Default: 100.
    pub page_size: u32,
    /// Maximum pages per connection, at least 1. Default: 500.
    pub max_pages: usize,
    /// Maximum repository requests in flight, at least 1. Default: 4.
    pub max_concurrent_repositories: usize,
}
impl Default for FetchOptions {
    fn default() -> Self {
        Self {
            page_size: 100,
            max_pages: 500,
            max_concurrent_repositories: 4,
        }
    }
}
impl FetchOptions {
    pub(crate) fn validate(self) -> Result<Self> {
        let invalid = if !(1..=100).contains(&self.page_size) {
            Some("page_size must be between 1 and 100")
        } else if self.max_pages == 0 {
            Some("max_pages must be greater than zero")
        } else if self.max_concurrent_repositories == 0 {
            Some("max_concurrent_repositories must be greater than zero")
        } else if self
            .max_pages
            .checked_mul(self.page_size as usize)
            .is_none()
        {
            Some("max_pages multiplied by page_size must fit in usize")
        } else {
            None
        };
        if let Some(message) = invalid {
            return Err(GitHubError::InvalidInput(message.into()));
        }
        Ok(self)
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Connection<T> {
    pub nodes: Vec<T>,
    pub total_count: usize,
    pub page_info: PageInfo,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PageInfo {
    pub has_next_page: bool,
    pub end_cursor: Option<String>,
}
#[derive(Default)]
pub(crate) struct Tracker {
    pub cursor: Option<String>,
    total: Option<usize>,
    ids: HashSet<String>,
    cursors: HashSet<String>,
    pages: usize,
}
impl Tracker {
    pub fn accept<T>(
        &mut self,
        page: &Connection<T>,
        options: FetchOptions,
        id: impl Fn(&T) -> &str,
    ) -> Result<()> {
        let fail = || GitHubError::PaginationError("Incomplete or inconsistent connection".into());
        self.pages += 1;
        if self.pages > options.max_pages
            || page.nodes.len() > options.page_size as usize
            || page.total_count > options.max_pages * options.page_size as usize
            || self.total.is_some_and(|total| total != page.total_count)
        {
            return Err(fail());
        }
        self.total = Some(page.total_count);
        for node in &page.nodes {
            let id = id(node);
            if id.is_empty() || !self.ids.insert(id.to_owned()) {
                return Err(fail());
            }
        }
        if self.ids.len() > page.total_count {
            return Err(fail());
        }
        if page.page_info.has_next_page {
            let cursor = page
                .page_info
                .end_cursor
                .as_deref()
                .filter(|c| !c.is_empty())
                .ok_or_else(fail)?;
            if page.nodes.is_empty()
                || self.ids.len() >= page.total_count
                || self.pages >= options.max_pages
                || !self.cursors.insert(cursor.to_owned())
            {
                return Err(fail());
            }
            self.cursor = Some(cursor.to_owned());
        } else {
            if self.ids.len() != page.total_count {
                return Err(fail());
            }
            self.cursor = None;
        }
        Ok(())
    }
}

// Concrete callback type for collection calls that do not request progress.
pub(crate) type NoProgress<T> = fn(&T) -> std::future::Ready<Result<()>>;
