use super::{
    PromptHead, SlashCommand, alias, argument, command_specs, joined_prompt, parse_command,
    prompt_head,
};

#[test]
fn an_alias_runs_its_target_quietly() {
    assert_eq!(alias("/rollback 3").as_deref(), Some("/undo files 3"));
    assert_eq!(alias("/quit").as_deref(), Some("/exit"));
    assert_eq!(parse_command("/rollback 3"), parse_command("/undo files 3"));
}

#[test]
fn a_current_command_is_not_rewritten() {
    assert!(alias("/undo").is_none());
    assert_eq!(parse_command("/login"), Some(SlashCommand::Login));
}

#[test]
fn approve_is_not_a_command() {
    assert_eq!(parse_command("/approve"), None);
    assert!(alias("/approve").is_none());
    let permissions = command_specs()
        .iter()
        .find(|spec| spec.name == "/permissions")
        .expect("permissions");
    assert_eq!(permissions.usage, "[write]");
    assert_eq!(permissions.description, "extra write paths for --strict");
}

#[test]
fn a_server_prompt_line_is_a_prompt_not_a_command() {
    assert_eq!(parse_command("/github:review 42"), None);
    let workspace = tempfile::tempdir().expect("workspace");
    assert_eq!(argument("/github:review 42", workspace.path()), Ok(None));
    drop(argument("/nosuch 42", workspace.path()).unwrap_err());
}

#[test]
fn an_mcp_prompt_runs_in_either_spelling() {
    let workspace = tempfile::tempdir().expect("workspace");
    assert_eq!(parse_command("/mcp__github__pr_review 456"), None);
    assert_eq!(
        argument("/mcp__github__pr_review 456", workspace.path()),
        Ok(None)
    );
    assert_eq!(
        prompt_head("/github:pr_review"),
        Some(PromptHead::Named {
            server: "github",
            prompt: "pr_review"
        })
    );
    assert_eq!(
        prompt_head("/mcp__github__pr_review"),
        Some(PromptHead::Joined("github__pr_review"))
    );
    assert_eq!(prompt_head("/mcp__github"), None);
    assert_eq!(
        joined_prompt("github__pr_review", "github"),
        Some("pr_review")
    );
    assert_eq!(joined_prompt("my_srv__list", "my.srv"), Some("list"));
    assert_eq!(joined_prompt("github__", "github"), None);
    assert_eq!(joined_prompt("gitlab__list", "github"), None);
}
