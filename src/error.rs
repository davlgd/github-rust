use crate::github::types::GraphQLError;
use std::{error::Error, fmt};

/// Metadata needed to decide when to retry a rate-limited request.
#[derive(Debug, Clone, Default)]
pub struct RateLimitDetails {
    pub message: String,
    pub status: Option<u16>,
    pub resource: Option<String>,
    pub remaining: Option<u64>,
    pub reset: Option<u64>,
    /// Original Retry-After header, in seconds or HTTP-date form.
    pub retry_after: Option<String>,
    pub request_id: Option<String>,
    pub graphql_errors: Vec<GraphQLError>,
}

impl fmt::Display for RateLimitDetails {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(f)
    }
}

/// A response decoding failure, retaining its original cause when available.
#[derive(Debug)]
pub enum DecodeError {
    Http(reqwest::Error),
    Json(serde_json::Error),
    InvalidResponse(String),
}

impl From<String> for DecodeError {
    fn from(message: String) -> Self {
        Self::InvalidResponse(message)
    }
}
impl From<&str> for DecodeError {
    fn from(message: &str) -> Self {
        Self::InvalidResponse(message.into())
    }
}
impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http(error) => error.fmt(f),
            Self::Json(error) => error.fmt(f),
            Self::InvalidResponse(message) => message.fmt(f),
        }
    }
}
impl Error for DecodeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Http(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::InvalidResponse(_) => None,
        }
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum GitHubError {
    NetworkError(reqwest::Error),
    AuthenticationError(String),
    AccessDeniedError(String),
    NotFoundError(String),
    RateLimitError(Box<RateLimitDetails>),
    AccessBlockedError(String),
    DmcaBlockedError(String),
    InvalidInput(String),
    PaginationError(String),
    ApiError { status: u16, message: String },
    GraphQLError(Vec<GraphQLError>),
    ParseError(DecodeError),
    ConfigError(String),
}

impl fmt::Display for GitHubError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NetworkError(error) => write!(f, "Network error: {error}"),
            Self::AuthenticationError(message) => write!(f, "Authentication error: {message}"),
            Self::AccessDeniedError(message) => write!(f, "Access denied: {message}"),
            Self::NotFoundError(message) => write!(f, "Not found: {message}"),
            Self::RateLimitError(details) => write!(f, "Rate limit exceeded: {details}"),
            Self::AccessBlockedError(message) => write!(f, "Access blocked: {message}"),
            Self::DmcaBlockedError(message) => {
                write!(f, "Unavailable for legal reasons: {message}")
            }
            Self::PaginationError(message) => write!(f, "Pagination error: {message}"),
            Self::InvalidInput(message) => write!(f, "Invalid input: {message}"),
            Self::ApiError { status, message } => write!(f, "GitHub API error {status}: {message}"),
            Self::GraphQLError(errors) => write!(
                f,
                "GraphQL errors: {}",
                errors
                    .iter()
                    .map(|error| error.message.as_str())
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
            Self::ParseError(error) => write!(f, "Failed to parse response: {error}"),
            Self::ConfigError(message) => write!(f, "Configuration error: {message}"),
        }
    }
}

impl Error for GitHubError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::NetworkError(error) => Some(error),
            Self::ParseError(error) => Some(error),
            _ => None,
        }
    }
}

impl From<reqwest::Error> for GitHubError {
    fn from(error: reqwest::Error) -> Self {
        if error.is_decode() {
            Self::ParseError(DecodeError::Http(error))
        } else {
            Self::NetworkError(error)
        }
    }
}
impl From<serde_json::Error> for GitHubError {
    fn from(error: serde_json::Error) -> Self {
        Self::ParseError(DecodeError::Json(error))
    }
}

pub type Result<T> = std::result::Result<T, GitHubError>;

impl GitHubError {
    /// Attach the requested repository identifier to a resource-not-found error.
    pub(crate) fn with_repository_context(self, owner: &str, name: &str) -> Self {
        match self {
            Self::NotFoundError(_) => Self::NotFoundError(format!("{owner}/{name}")),
            error => error,
        }
    }
}
