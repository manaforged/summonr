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
        vec!["/model"]
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
    assert_eq!(tool_arg("not-json"), "");
}

#[test]
fn compact_formatters_keep_the_useful_tail() {
    assert_eq!(fmt_tokens(999), "999");
    assert_eq!(fmt_tokens(1_500), "1.5k");
    assert_eq!(short_session("session-12345"), "12345");
    assert_eq!(last_line("one\n\ntwo\n"), "two");
    assert_eq!(tail_lines("one\ntwo\nthree", 2), "two\nthree");
}

#[test]
fn help_lists_the_live_composer_bindings() {
    let help = help_text();
    for binding in [
        "/model",
        "enter send/steer",
        "tab complete/queue",
        "ctrl+j newline",
        "shift+tab mode",
        "ctrl+o settings",
        "ctrl+g editor",
    ] {
        assert!(help.contains(binding), "missing `{binding}` in:\n{help}");
    }
    assert!(
        !help.contains("ctrl+enter send"),
        "stale binding in:\n{help}"
    );
}
