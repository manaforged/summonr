use super::{SlashCommand, argument, parse_command};

#[test]
fn arguments_preserve_paths_and_text_and_reject_unsupported_input() {
    for line in [
        r#"/view "NOTES with spaces.md""#,
        "/view 'NOTES with spaces.md'",
        "/view NOTES with spaces.md",
    ] {
        assert_eq!(argument(line), Ok(Some("NOTES with spaces.md")));
    }
    assert_eq!(
        argument(r#"/view "C:\Users\A B\notes.md""#),
        Ok(Some(r"C:\Users\A B\notes.md"))
    );
    assert_eq!(
        argument(r#"/layout "chat | files 30%""#),
        Ok(Some("chat | files 30%"))
    );
    assert_eq!(argument("  /plan  accept  "), Ok(None));
    assert_eq!(argument("explain /plan to me"), Ok(None));
    assert!(
        argument("  /compactt preserve this draft")
            .unwrap_err()
            .contains("Unknown command `/compactt`")
    );
    assert_eq!(
        argument(r#"/goal Explain "these quotes""#),
        Ok(Some(r#"Explain "these quotes""#))
    );
    for line in [
        "/theme nonsense",
        "/tools full",
        "/plan accept now",
        "/quit later",
    ] {
        assert!(argument(line).unwrap_err().contains("takes no arguments"));
    }
    assert!(
        argument("/export \"unfinished.md")
            .unwrap_err()
            .contains("Close the quoted argument")
    );
}

#[test]
fn commands_with_arguments_resolve_by_prefix() {
    assert_eq!(parse_command("/effort low"), Some(SlashCommand::Effort));
    assert_eq!(
        parse_command("/name widget spike"),
        Some(SlashCommand::Name)
    );
    assert_eq!(parse_command("/steps PLAN.md"), Some(SlashCommand::Steps));
    assert_eq!(parse_command("/goal clear"), Some(SlashCommand::Goal));
    assert_eq!(parse_command("/model grok-4.6"), Some(SlashCommand::Model));
    assert_eq!(
        parse_command("/workflow set judge grok"),
        Some(SlashCommand::Workflow)
    );
    assert_eq!(
        parse_command("/plan accept"),
        Some(SlashCommand::PlanAccept)
    );
    assert_eq!(
        parse_command("/plan reject"),
        Some(SlashCommand::PlanReject)
    );
    assert_eq!(parse_command("  /plan  "), Some(SlashCommand::Plan));
    assert_eq!(parse_command("/compact"), Some(SlashCommand::Compact));
    assert_eq!(parse_command("/copy"), Some(SlashCommand::Copy));
    assert_eq!(parse_command("/net once"), Some(SlashCommand::Network));
    assert_eq!(parse_command("/write ~/.cargo"), Some(SlashCommand::Write));
    assert_eq!(parse_command("/approve on"), Some(SlashCommand::Approve));
    assert_eq!(parse_command("/net"), Some(SlashCommand::Network));
    assert_eq!(parse_command("/resume"), Some(SlashCommand::Resume));
    assert_eq!(
        parse_command("/resume widget spike"),
        Some(SlashCommand::Resume)
    );
    assert_eq!(parse_command("/bogus x"), None);
    assert_eq!(parse_command("fix /tmp/x"), None);
    assert_eq!(parse_command(""), None);
}
