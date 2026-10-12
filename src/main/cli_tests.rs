use super::*;

#[test]
fn room_is_rejected_instead_of_run_as_a_prompt() {
    match parse_from(["room", "serve"].map(String::from)) {
        Ok(args) => panic!("`room serve` parsed as the prompt {:?}", args.prompt),
        Err(error) => assert!(error.to_string().contains("summonr mail"), "{error}"),
    }
}

#[test]
fn inline_turns_on_inline_mode() {
    let args = parse_from(["--inline"].map(String::from)).unwrap();
    assert!(args.inline);
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
