//! Shared HTTP and GraphQL response handling.
use crate::{error::*, github::types::GraphQLResponse};
use reqwest::{Response, header::HeaderMap};
use serde::de::DeserializeOwned;

fn header(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

fn rate_details(status: u16, headers: &HeaderMap) -> RateLimitDetails {
    RateLimitDetails {
        status: Some(status),
        resource: header(headers, "x-ratelimit-resource"),
        remaining: header(headers, "x-ratelimit-remaining").and_then(|value| value.parse().ok()),
        reset: header(headers, "x-ratelimit-reset").and_then(|value| value.parse().ok()),
        retry_after: header(headers, "retry-after"),
        request_id: header(headers, "x-github-request-id"),
        ..Default::default()
    }
}

pub(crate) async fn check(response: Response) -> Result<Response> {
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status().as_u16();
    let mut rate = rate_details(status, response.headers());
    let body = response.text().await?;
    let message = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|value| value["message"].as_str().map(str::to_owned))
        .unwrap_or(body);
    let lower = message.to_lowercase();
    let error = match status {
        401 => GitHubError::AuthenticationError(message),
        403 | 429
            if status == 429
                || rate.remaining == Some(0)
                || rate.retry_after.is_some()
                || lower.contains("rate limit") =>
        {
            rate.message = message;
            GitHubError::RateLimitError(Box::new(rate))
        }
        403 if lower.contains("access blocked") => GitHubError::AccessBlockedError(message),
        403 => GitHubError::AccessDeniedError(message),
        404 => GitHubError::NotFoundError(message),
        451 => GitHubError::DmcaBlockedError(message),
        _ => GitHubError::ApiError { status, message },
    };
    Err(error)
}

pub(crate) async fn graphql<T: DeserializeOwned>(response: Response) -> Result<T> {
    let response = check(response).await?;
    let mut rate = rate_details(response.status().as_u16(), response.headers());
    let response: GraphQLResponse<T> = response.json().await?;
    let errors = response.errors.unwrap_or_default();
    if !errors.is_empty() {
        if errors
            .iter()
            .any(|error| error.error_type.as_deref() == Some("RATE_LIMITED"))
        {
            rate.message = errors
                .iter()
                .map(|error| error.message.as_str())
                .collect::<Vec<_>>()
                .join("; ");
            rate.resource.get_or_insert_with(|| "graphql".into());
            rate.graphql_errors = errors;
            return Err(GitHubError::RateLimitError(Box::new(rate)));
        }
        return Err(GitHubError::GraphQLError(errors));
    }
    response
        .data
        .ok_or_else(|| GitHubError::ParseError("No data in GraphQL response".into()))
}
