use super::{Args, parse_from};

fn args(tokens: &[&str]) -> Args {
    parse_from(tokens.iter().map(|token| (*token).to_string())).expect("parse")
}

#[test]
fn workspace_instructions_load_by_default() {
    assert!(args(&[]).trust);
    assert!(!args(&["--no-trust"]).trust);
}

#[test]
fn first_token_named_endpoint_is_the_target() {
    let parsed = args(&["grok", "-p", "hello"]);
    assert_eq!(parsed.endpoint.as_deref(), Some("grok"));
    assert!(parsed.print);
    assert_eq!(parsed.prompt, ["hello"]);
}

#[test]
fn anthropic_token_is_a_named_seat() {
    let parsed = args(&["anthropic", "-p", "hi"]);
    assert_eq!(parsed.endpoint.as_deref(), Some("anthropic"));
    assert!(parsed.print);
}

#[test]
fn kimi_token_matches_minus_e() {
    let parsed = args(&["kimi"]);
    assert_eq!(parsed.endpoint.as_deref(), Some("kimi"));
    assert!(parsed.prompt.is_empty());
}

#[test]
fn minus_e_wins_over_later_name() {
    let parsed = args(&["-e", "kimi", "grok"]);
    assert_eq!(parsed.endpoint.as_deref(), Some("kimi"));
    assert_eq!(parsed.prompt, ["grok"]);
}

#[test]
fn ordinary_prompt_stays_a_prompt() {
    let parsed = args(&["summarize-this-repo"]);
    assert!(parsed.endpoint.is_none());
    assert_eq!(parsed.prompt, ["summarize-this-repo"]);
}

#[test]
fn login_subcommand_is_not_an_endpoint() {
    let parsed = args(&["login", "grok"]);
    assert!(parsed.login);
    assert_eq!(parsed.endpoint.as_deref(), Some("grok"));
}

#[test]
fn login_copilot_is_the_device_preset() {
    let parsed = args(&["login", "copilot"]);
    assert!(parsed.login);
    assert_eq!(parsed.endpoint.as_deref(), Some("copilot"));
}

#[test]
fn sessions_subcommand_is_not_an_endpoint() {
    let parsed = args(&["sessions"]);
    assert!(parsed.list_sessions);
    assert!(parsed.endpoint.is_none());
}

#[test]
fn worktrees_prune_is_not_an_endpoint() {
    let parsed = args(&["worktrees", "prune"]);
    assert!(parsed.list_worktrees);
    assert!(parsed.prune_worktrees);
}

#[test]
fn here_and_worktree_conflict() {
    let error = super::parse_from(["--here", "--worktree"].into_iter().map(str::to_string))
        .expect_err("conflict");
    assert!(error.to_string().contains("--here"));
}
