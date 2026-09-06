use github_rust::{GitHubClient, GitHubError};
use std::process::Command;

#[test]
fn builder_validates_tokens_and_endpoints() {
    assert!(!GitHubClient::builder().build().unwrap().has_token());
    assert!(
        GitHubClient::builder()
            .token("test_token".into())
            .build()
            .unwrap()
            .has_token()
    );
    for token in ["", "   ", "\t\n", "invalid\ntoken"] {
        assert!(matches!(
            GitHubClient::builder().token(token.into()).build(),
            Err(GitHubError::ConfigError(_))
        ));
    }
    for endpoint in [
        "ftp://api.example.com",
        "https://user:secret@api.example.com",
        "https://api.example.com/?debug=1",
        "https://api.example.com/#fragment",
        "https://",
        "not a url",
    ] {
        for builder in [
            GitHubClient::builder().rest_url(endpoint),
            GitHubClient::builder().graphql_url(endpoint),
        ] {
            assert!(
                matches!(builder.build(), Err(GitHubError::ConfigError(_))),
                "{endpoint} must be rejected"
            );
        }
    }
    assert!(
        GitHubClient::builder()
            .rest_url("http://localhost:8080/api/v3/")
            .graphql_url("http://localhost:8080/api/graphql")
            .build()
            .is_ok()
    );
}

// Environment detection runs in a child process so no test mutates global variables.
#[test]
fn environment_token_detection_ignores_blank_values() {
    const CHILD: &str = "GITHUB_RUST_EXPECT_TOKEN";
    if let Some(expected) = std::env::var_os(CHILD) {
        let client = GitHubClient::new().expect("blank tokens must not fail construction");
        assert_eq!(client.has_token(), expected == "1");
        return;
    }
    for (token, expected) in [
        (None, "0"),
        (Some(""), "0"),
        (Some("   "), "0"),
        (Some("\n"), "0"),
        (Some("ghp_example"), "1"),
    ] {
        let mut child = Command::new(std::env::current_exe().unwrap());
        child
            .args([
                "--exact",
                "client::environment_token_detection_ignores_blank_values",
            ])
            .env(CHILD, expected)
            .env_remove("GITHUB_TOKEN");
        if let Some(token) = token {
            child.env("GITHUB_TOKEN", token);
        }
        let output = child.output().unwrap();
        assert!(
            output.status.success(),
            "GITHUB_TOKEN={token:?} expected has_token={expected}:\n{}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}
