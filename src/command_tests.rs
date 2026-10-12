use super::{SlashCommand, argument, command_specs, parse_command, renamed};

#[test]
fn an_old_command_name_runs_its_replacement_and_says_so() {
    let (line, note) = renamed("/rollback 3").unwrap();
    assert_eq!(line, "/undo files 3");
    assert_eq!(note.as_deref(), Some("renamed: use /undo files"));
    assert_eq!(parse_command("/rollback 3"), parse_command("/undo files 3"));
}

#[test]
fn a_current_command_is_not_rewritten() {
    assert!(renamed("/undo").is_none());
    assert_eq!(parse_command("/login"), Some(SlashCommand::Login));
}

#[test]
fn approve_is_not_a_command() {
    assert_eq!(parse_command("/approve"), None);
    assert!(renamed("/approve").is_none());
    let permissions = command_specs()
        .iter()
        .find(|spec| spec.name == "/permissions")
        .expect("permissions");
    assert_eq!(permissions.usage, "[net|write]");
    assert_eq!(permissions.description, "sandbox network and write paths");
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
