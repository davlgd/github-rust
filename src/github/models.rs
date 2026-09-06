//! Public repository models, independent of REST and GraphQL response envelopes.
use crate::github::types::{Language, License};
use serde::{Deserialize, Serialize};

/// Repository metadata. Optional counts are unknown when an API cannot supply them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repository {
    pub node_id: String,
    pub database_id: Option<u64>,
    pub name: String,
    pub name_with_owner: String,
    pub description: Option<String>,
    pub url: String,
    pub homepage_url: Option<String>,
    /// Creation timestamp supplied by GitHub, in ISO 8601 format.
    pub created_at: String,
    /// Last metadata update timestamp, in ISO 8601 format.
    pub updated_at: String,
    /// Last push timestamp in ISO 8601 format, when available.
    pub pushed_at: Option<String>,
    pub is_private: bool,
    pub is_fork: bool,
    pub is_archived: bool,
    pub stargazer_count: u32,
    pub fork_count: u32,
    /// Users subscribed to notifications, not users who starred the repository.
    pub watcher_count: Option<u32>,
    /// Open issues, excluding pull requests. Unavailable from the REST repository response.
    pub open_issue_count: Option<u32>,
    /// Pull requests in all states. Unavailable from the REST repository response.
    pub pull_request_count: Option<u32>,
    /// Unavailable from the REST repository response.
    pub release_count: Option<u32>,
    pub primary_language: Option<Language>,
    /// None if the optional language breakdown could not be retrieved.
    pub languages: Option<Vec<LanguageUsage>>,
    /// False when languages are unavailable or the GraphQL connection has more pages.
    pub languages_complete: bool,
    pub license_info: Option<License>,
    pub default_branch: Option<String>,
    pub topics: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageUsage {
    pub language: Language,
    pub bytes: u64,
}

/// Repository summary returned by search.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchRepository {
    pub node_id: String,
    pub database_id: Option<u64>,
    pub name: String,
    pub name_with_owner: String,
    pub description: Option<String>,
    pub url: String,
    pub stargazer_count: u32,
    pub fork_count: u32,
    /// Creation timestamp supplied by GitHub, in ISO 8601 format.
    pub created_at: String,
    /// Last metadata update timestamp, in ISO 8601 format.
    pub updated_at: String,
    /// Last push timestamp in ISO 8601 format, when available.
    pub pushed_at: Option<String>,
    pub primary_language: Option<Language>,
    pub license_info: Option<License>,
    pub topics: Vec<String>,
}

macro_rules! repository_helpers {
    ($model:ty) => {
        impl $model {
            pub fn language(&self) -> Option<&str> {
                self.primary_language
                    .as_ref()
                    .map(|language| language.name.as_str())
            }
            pub fn license(&self) -> Option<&str> {
                self.license_info
                    .as_ref()
                    .map(|license| license.name.as_str())
            }
            pub fn license_spdx(&self) -> Option<&str> {
                self.license_info
                    .as_ref()
                    .and_then(|license| license.spdx_id.as_deref())
            }
            pub fn topics(&self) -> Vec<&str> {
                self.topics.iter().map(String::as_str).collect()
            }
            pub fn owner(&self) -> &str {
                self.name_with_owner
                    .split_once('/')
                    .map_or(self.name_with_owner.as_str(), |(owner, _)| owner)
            }
        }
    };
}
repository_helpers!(Repository);
repository_helpers!(SearchRepository);

impl Repository {
    pub fn default_branch(&self) -> Option<&str> {
        self.default_branch.as_deref()
    }
    pub fn open_issues(&self) -> Option<u32> {
        self.open_issue_count
    }
    pub fn watcher_count(&self) -> Option<u32> {
        self.watcher_count
    }
}
