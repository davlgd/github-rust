pub mod accounts;
pub mod client;
pub mod graphql;
pub mod models;
mod pagination;
mod response;
pub mod rest;
pub mod search;
pub mod service;
pub mod types;

pub use client::{GitHubClient, GitHubClientBuilder, RateLimit, RateLimits};
pub use models::{LanguageUsage, Repository};
pub use rest::UserProfile;
pub use search::{SearchRepository, search_repositories};
pub use service::{FallbackPolicy, GitHubService};
pub use types::*;

pub use accounts::{Account, OwnedRepositories, RepositoryPage, RepositorySummary, Viewer};
pub use pagination::FetchOptions;

pub mod work_items;
pub use work_items::{
    Actor, Issue, Label, PullRequest, RepositoryCoordinates, RepositoryReference, ReviewDecision,
    WorkItemPage,
};
