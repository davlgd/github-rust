use std::fmt;

#[derive(Debug)]
#[non_exhaustive]
pub enum GitHubError {
    NetworkError(String),
    AuthenticationError(String),
    NotFoundError(String),
    RateLimitError(String),
    AccessBlockedError(String),
    DmcaBlockedError(String),
    InvalidInput(String),
    ApiError { status: u16, message: String },
    ParseError(String),
    ConfigError(String),
}

impl fmt::Display for GitHubError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GitHubError::NetworkError(msg) => {
                write!(
                    f,
                    "Network error: {}. Please check your internet connection",
                    msg
                )
            }
            GitHubError::AuthenticationError(msg) => {
                write!(
                    f,
                    "Authentication error: {}. Please check your GitHub token",
                    msg
                )
            }
            GitHubError::NotFoundError(msg) => {
                write!(
                    f,
                    "Repository not found: {}. Please verify the owner and repository name",
                    msg
                )
            }
            GitHubError::RateLimitError(msg) => {
                write!(
                    f,
                    "Rate limit exceeded: {}. Please wait or use a GitHub token for higher limits",
                    msg
                )
            }
            GitHubError::AccessBlockedError(msg) => {
                write!(
                    f,
                    "Repository access blocked: {}. This repository is inaccessible due to policy violations",
                    msg
                )
            }
            GitHubError::DmcaBlockedError(msg) => {
                write!(
                    f,
                    "Repository blocked for legal reasons (DMCA): {}. This repository is permanently inaccessible",
                    msg
                )
            }
            GitHubError::InvalidInput(msg) => {
                write!(f, "Invalid input: {}. Use format 'owner/repo'", msg)
            }
            GitHubError::ApiError { status, message } => {
                write!(f, "GitHub API error {}: {}", status, message)
            }
            GitHubError::ParseError(msg) => {
                write!(f, "Failed to parse response: {}", msg)
            }
            GitHubError::ConfigError(msg) => {
                write!(f, "Configuration error: {}", msg)
            }
        }
    }
}

impl std::error::Error for GitHubError {}

impl From<reqwest::Error> for GitHubError {
    fn from(error: reqwest::Error) -> Self {
        if error.is_timeout() {
            GitHubError::NetworkError("Request timeout".to_string())
        } else if error.is_connect() {
            GitHubError::NetworkError("Connection failed".to_string())
        } else {
            GitHubError::NetworkError(error.to_string())
        }
    }
}

impl From<serde_json::Error> for GitHubError {
    fn from(error: serde_json::Error) -> Self {
        GitHubError::ParseError(error.to_string())
    }
}

pub type Result<T> = std::result::Result<T, GitHubError>;
