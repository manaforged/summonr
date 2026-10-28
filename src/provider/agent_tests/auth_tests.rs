use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};

use crate::provider::Client;

const STUB: &str = r#"#!/bin/sh
printf '%s\n' "$*" >> "$0.calls"
case "$*" in
  'auth status --json') cat "$0.status" ;;
  'auth login --claudeai')
    printf 'Opening browser to sign in…\nIf the browser did not open, visit: https://auth.example.test/login\nPaste code here if prompted > '
    case $(cat "$0.mode") in
      fail) exit 7 ;;
      code) IFS= read -r code; printf '%s' "$code" > "$0.code" ;;
      browser) : ;;
    esac
    printf '%s\n' '{"loggedIn":true,"authMethod":"claude.ai"}' > "$0.status"
    ;;
  *) exit 22 ;;
esac
"#;

fn endpoint(dir: &std::path::Path, mode: &str) -> crate::Endpoint {
    let program = dir.join("claude");
    std::fs::write(&program, STUB).expect("write the Claude Code fixture");
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755))
        .expect("make the Claude Code fixture executable");
    std::fs::write(dir.join("claude.mode"), mode).expect("set the fixture login behavior");
    std::fs::write(
        dir.join("claude.status"),
        r#"{"loggedIn":false,"authMethod":null}"#,
    )
    .expect("write the signed-out fixture status");
    let mut endpoint = crate::endpoint::resolve(Some("claude"), None, crate::Need::Config)
        .expect("resolve the built-in Claude Code route");
    endpoint.base_url = Some(program.display().to_string());
    endpoint
}

#[tokio::test]
async fn claude_code_auth_sends_the_browser_code_to_claude_not_the_key_store() {
    let dir = tempfile::tempdir().expect("create an isolated Claude Code fixture");
    let endpoint = endpoint(dir.path(), "code");
    let said = Arc::new(Mutex::new(Vec::new()));
    let lines = said.clone();
    let mut prompt = summonr_auth::Prompt::new(
        move |line| {
            lines
                .lock()
                .expect("prompt recorder is not poisoned")
                .push(line.to_string())
        },
        |question| {
            Box::pin(async move {
                assert_eq!(question, "Paste the code if the browser shows one");
                Some("test-code".into())
            })
        },
    );
    Client::login(&endpoint, &mut prompt)
        .await
        .expect("the fixture completes Claude Code sign-in");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("claude.code"))
            .expect("read the code received by Claude Code"),
        "test-code"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("claude.calls"))
            .expect("read the commands sent to Claude Code"),
        "auth login --claudeai\nauth status --json\n"
    );
    assert!(
        said.lock()
            .expect("prompt recorder is not poisoned")
            .iter()
            .any(|line| line == "open https://auth.example.test/login")
    );
}

#[tokio::test]
async fn claude_code_auth_browser_completion_does_not_require_a_pasted_code() {
    let dir = tempfile::tempdir().expect("create an isolated Claude Code fixture");
    let endpoint = endpoint(dir.path(), "browser");
    let mut prompt = summonr_auth::Prompt::new(|_| {}, |_| Box::pin(std::future::pending()));
    Client::login(&endpoint, &mut prompt)
        .await
        .expect("the fixture completes Claude Code sign-in");
    assert!(!dir.path().join("claude.code").exists());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("claude.calls"))
            .expect("read the commands sent to Claude Code"),
        "auth login --claudeai\nauth status --json\n"
    );
}

#[tokio::test]
async fn claude_code_auth_failed_or_cancelled_login_is_not_successful() {
    for mode in ["fail", "code"] {
        let dir = tempfile::tempdir().expect("create an isolated Claude Code fixture");
        let endpoint = endpoint(dir.path(), mode);
        let mut prompt = summonr_auth::Prompt::new(|_| {}, |_| Box::pin(async { None }));
        assert!(Client::login(&endpoint, &mut prompt).await.is_err());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("claude.calls"))
                .expect("read the commands sent to Claude Code"),
            "auth login --claudeai\n"
        );
    }
}

#[tokio::test]
async fn claude_code_auth_discovery_requires_a_subscription_not_just_a_binary_or_api_key() {
    let dir = tempfile::tempdir().expect("create an isolated Claude Code fixture");
    let endpoint = endpoint(dir.path(), "browser");
    for status in [
        r#"{"loggedIn":false,"authMethod":null}"#,
        r#"{"loggedIn":true,"authMethod":"api_key"}"#,
    ] {
        std::fs::write(dir.path().join("claude.status"), status)
            .expect("set the fixture authentication method");
        let error = Client::discover(&endpoint)
            .await
            .expect_err("a subscription is required before listing Claude models");
        assert_eq!(Client::state(&error), crate::endpoint::State::NeedsKey);
    }
    std::fs::write(
        dir.path().join("claude.status"),
        r#"{"loggedIn":true,"authMethod":"claude.ai"}"#,
    )
    .expect("set the fixture subscription login");
    assert!(
        !Client::discover(&endpoint)
            .await
            .expect("a signed-in subscription lists models")
            .is_empty()
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("claude.calls"))
            .expect("read the commands sent to Claude Code"),
        "auth status --json\nauth status --json\nauth status --json\n"
    );
}

#[tokio::test]
async fn claude_code_auth_missing_binary_explains_what_to_install() {
    let dir = tempfile::tempdir().expect("create an isolated Claude Code fixture");
    let mut endpoint = endpoint(dir.path(), "browser");
    endpoint.base_url = Some(dir.path().join("not-installed").display().to_string());
    let mut prompt = summonr_auth::Prompt::new(|_| {}, |_| Box::pin(async { None }));
    let error = Client::login(&endpoint, &mut prompt)
        .await
        .expect_err("a missing Claude Code binary cannot sign in");
    assert!(error.to_string().contains("install Claude Code"), "{error}");
}
