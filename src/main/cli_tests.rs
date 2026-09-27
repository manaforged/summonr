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
