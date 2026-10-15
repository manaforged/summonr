use super::{SlashCommand, alias, argument, command_specs, parse_command};

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
    assert_eq!(argument("/github:review 42"), Ok(None));
    drop(argument("/nosuch 42").unwrap_err());
}

#[test]
fn every_command_description_starts_in_lower_case() {
    for spec in command_specs() {
        let first = spec.description.split_whitespace().next().unwrap_or("");
        let acronym = first.chars().all(|c| !c.is_lowercase());
        assert!(
            acronym || first.starts_with(|c: char| !c.is_uppercase()),
            "{}: {}",
            spec.name,
            spec.description
        );
    }
}
