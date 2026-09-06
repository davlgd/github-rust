use std::time::Duration;

pub const GITHUB_API_URL: &str = "https://api.github.com";
pub const GITHUB_GRAPHQL_URL: &str = "https://api.github.com/graphql";
pub const USER_AGENT: &str = "github-rust/0.1.0";
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
pub const MAX_RETRIES: u32 = 3;

pub const GRAPHQL_REPOSITORY_QUERY: &str = r#"
query($owner: String!, $name: String!) {
  repository(owner: $owner, name: $name) {
    id
    databaseId
    name
    nameWithOwner
    description
    url
    homepageUrl
    createdAt
    updatedAt
    pushedAt
    isPrivate
    isFork
    isArchived
    stargazerCount
    forkCount
    watchers {
      totalCount
    }
    issues(states: OPEN) {
      totalCount
    }
    pullRequests {
      totalCount
    }
    releases {
      totalCount
    }
    primaryLanguage {
      name
      color
    }
    languages(first: 100, orderBy: {field: SIZE, direction: DESC}) {
      pageInfo { hasNextPage }
      edges {
        size
        node {
          name
          color
        }
      }
    }
    licenseInfo {
      name
      spdxId
    }
    defaultBranchRef {
      name
    }
    repositoryTopics(first: 100) {
      edges {
        node {
          topic {
            name
          }
        }
      }
    }
  }
}
"#;

pub const GRAPHQL_SEARCH_REPOSITORIES_QUERY: &str = r#"
query($queryString: String!, $first: Int!, $after: String) {
  search(query: $queryString, type: REPOSITORY, first: $first, after: $after) {
    repositoryCount
    pageInfo {
      hasNextPage
      endCursor
    }
    edges {
      node {
        ... on Repository {
          id
          databaseId
          name
          nameWithOwner
          description
          url
          stargazerCount
          forkCount
          createdAt
          updatedAt
          pushedAt
          primaryLanguage {
            name
            color
          }
          licenseInfo {
            name
            spdxId
          }
          repositoryTopics(first: 100) {
            edges {
              node {
                topic {
                  name
                }
              }
            }
          }
        }
      }
    }
  }
}
"#;
