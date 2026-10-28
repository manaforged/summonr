use super::*;

#[test]
fn old_verbs_are_prompts_not_commands() {
    let args = parse_from(["auth"].map(String::from)).unwrap();
    assert!(!matches!(args.verb, Some(Verb::Login)));
    assert_eq!(args.prompt, ["auth"]);
    let args = parse_from(["logs"].map(String::from)).unwrap();
    assert!(!args.logs);
}

#[test]
fn goal_and_steps_together_are_refused() {
    let error = parse_from(["--goal", "x", "--steps", "y"].map(String::from))
        .err()
        .unwrap()
        .to_string();
    assert!(
        error.contains("--goal") && error.contains("--steps"),
        "{error}"
    );
}
