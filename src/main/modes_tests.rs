use super::slash;

#[test]
fn a_leading_slash_line_is_a_command_not_a_prompt() {
    assert_eq!(slash("/exit\n"), Some("/exit"));
    assert_eq!(slash("  /model grok"), Some("/model"));
    assert_eq!(slash("fix /tmp/x"), None);
    assert_eq!(slash("/var/log is full on the box, why"), None);
    assert_eq!(slash("/Users/x/repo has a bug"), None);
    assert_eq!(slash(""), None);
}
