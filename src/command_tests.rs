use super::{SlashCommand, parse_command, renamed};

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
