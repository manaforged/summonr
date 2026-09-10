use super::*;

#[test]
fn slash_table_is_unique_and_filters_prefixes() {
    let names: Vec<_> = crate::command_specs()
        .iter()
        .map(|spec| spec.name)
        .collect();
    let mut unique = names.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(names.len(), unique.len());
    assert_eq!(
        matching_commands("/plan"),
        vec![
            ("/plan", "toggle plan mode"),
            ("/plan accept", "keep the journaled plan and return to auto"),
            ("/plan reject", "leave plan mode without implementing"),
        ]
    );
    assert_eq!(
        matching_commands("/m")
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>(),
        vec!["/model", "/modules"]
    );
    assert_eq!(matching_commands("/mdl")[0].0, "/model");
    assert_eq!(matching_commands("/active")[0].0, "/model");
    assert_eq!(
        crate::parse_command("/model"),
        Some(crate::SlashCommand::Model)
    );
    assert_eq!(
        crate::parse_command("/model qwen-3"),
        Some(crate::SlashCommand::Model)
    );
    assert_eq!(crate::parse_command("/goalkeeper"), None);
    assert_eq!(
        crate::parse_command("/auth glm"),
        Some(crate::SlashCommand::Auth)
    );
    assert_eq!(
        crate::parse_command("/login copilot"),
        Some(crate::SlashCommand::Login)
    );
}

#[test]
fn tool_argument_uses_an_editable_summary() {
    assert_eq!(tool_arg(r#"{"path":"src/main.rs"}"#), "src/main.rs");
    assert_eq!(tool_arg(r#"{"command":"cargo test"}"#), "cargo test");
    assert_eq!(
        tool_arg(r#"{"command":"cd \"/Volumes/VAULT 4TB/dev/repo\" && grep -rn tag scripts/"}"#),
        "grep -rn tag scripts/"
    );
    assert_eq!(
        tool_arg(r#"{"command":"cd \"$(git rev-parse --show-toplevel)\"; ls -la"}"#),
        "ls -la"
    );
    assert_eq!(tool_arg(r#"{"command":"cd src"}"#), "cd src");
    assert_eq!(tool_arg("not-json"), "");
}

#[test]
fn compact_formatters_keep_the_useful_tail() {
    assert_eq!(fmt_tokens(999), "999");
    assert_eq!(fmt_tokens(1_500), "1.5k");
    assert_eq!(fmt_tokens(1_000_000), "1.0m");
    assert_eq!(short_session("session-12345"), "12345");
}

#[test]
fn context_reports_the_limit_and_percentage_used() {
    let round = Round {
        prompt: 64_000,
        completion: 1_536,
        reasoning: 8_000,
        cached: 32_000,
        prefix_hash: String::new(),
        cost: None,
    };

    assert_eq!(
        context(Some(&round), Some(262_144)),
        "65.5k / 262.1k · 25% used"
    );
    assert_eq!(context(None, Some(262_144)), "0 / 262.1k · 0% used");
    assert_eq!(context(Some(&round), None), "not set");
}

#[test]
fn help_lists_the_live_composer_bindings() {
    let help = help_text();
    for binding in [
        "/model",
        "/tools",
        "/reasoning",
        "send, or steer",
        "complete a /command",
        "switch between auto and plan",
        "hotkeys are editable",
        "twice on an empty draft quits",
        "!cmd",
        "alt+↑",
    ] {
        assert!(help.contains(binding), "missing `{binding}` in:\n{help}");
    }
    assert!(
        !help.contains("ctrl+enter send"),
        "stale binding in:\n{help}"
    );
}
