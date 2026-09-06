use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct GraphQLQuery<T> {
    pub query: String,
    pub variables: T,
}

#[derive(Deserialize)]
pub struct GraphQLResponse<T> {
    pub data: Option<T>,
    pub errors: Option<Vec<GraphQLError>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GraphQLError {
    pub message: String,
    #[serde(rename = "type")]
    pub error_type: Option<String>,
    pub path: Option<Vec<serde_json::Value>>,
    pub extensions: Option<serde_json::Value>,
}

#[derive(Deserialize, Serialize, Clone, Default, Debug)]
pub struct Language {
    pub name: String,
    pub color: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Default, Debug)]
pub struct License {
    pub name: String,
    /// Serialized as `spdxId`, retaining the shared GraphQL license format.
    #[serde(rename = "spdxId")]
    pub spdx_id: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Default, Debug)]
pub struct TopicConnection {
    pub edges: Vec<TopicEdge>,
}

#[derive(Deserialize, Serialize, Clone, Default, Debug)]
pub struct TopicEdge {
    pub node: TopicNode,
}

#[derive(Deserialize, Serialize, Clone, Default, Debug)]
pub struct TopicNode {
    pub topic: Topic,
}

#[derive(Deserialize, Serialize, Clone, Default, Debug)]
pub struct Topic {
    pub name: String,
}

#[derive(Deserialize, Serialize)]
pub struct TotalCount {
    #[serde(rename = "totalCount")]
    pub total_count: u32,
}

#[derive(Deserialize, Serialize)]
pub struct LanguageConnection {
    pub edges: Vec<LanguageEdge>,
}

#[derive(Deserialize, Serialize)]
pub struct LanguageEdge {
    pub size: u64,
    pub node: Language,
}

#[derive(Deserialize, Serialize)]
pub struct Branch {
    pub name: String,
}

/// GitHub user information
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct User {
    pub login: String,
    pub id: u64,
    pub node_id: String,
    pub avatar_url: String,
    pub gravatar_id: String,
    pub url: String,
    pub html_url: String,
    pub followers_url: String,
    pub following_url: String,
    pub gists_url: String,
    pub starred_url: String,
    pub subscriptions_url: String,
    pub organizations_url: String,
    pub repos_url: String,
    pub events_url: String,
    pub received_events_url: String,
    #[serde(rename = "type")]
    pub user_type: String,
    pub site_admin: bool,
}

/// Stargazer with timestamp when the repository was starred
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct StargazerWithDate {
    pub starred_at: String,
    pub user: User,
}
