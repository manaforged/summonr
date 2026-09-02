use super::{SlashCommand, parse_command};

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
    assert_eq!(parse_command("/resume"), Some(SlashCommand::Resume));
    assert_eq!(
        parse_command("/resume widget spike"),
        Some(SlashCommand::Resume)
    );
    assert_eq!(parse_command("/bogus x"), None);
    assert_eq!(parse_command("fix /tmp/x"), None);
    assert_eq!(parse_command(""), None);
}
