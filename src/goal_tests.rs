use super::*;

#[test]
fn verdict_parses_four_states_and_the_legacy_done_flag() {
    let verdict = parse_verdict("{\"verdict\": \"done\", \"reason\": \"tests pass\"}")
        .expect("valid verdict");
    assert_eq!(verdict.state, State::Done);
    assert_eq!(verdict.reason, "tests pass");

    let verdict = parse_verdict("{\"verdict\": \"continue\", \"reason\": \"two files remain\"}")
        .expect("valid verdict");
    assert_eq!(verdict.state, State::Continue);
    assert_eq!(verdict.reason, "two files remain");

    let verdict = parse_verdict("{\"verdict\": \"stalled\", \"reason\": \"same reply twice\"}")
        .expect("valid verdict");
    assert_eq!(verdict.state, State::Stalled);
    let verdict = parse_verdict("{\"verdict\": \"impossible\", \"reason\": \"no network\"}")
        .expect("valid verdict");
    assert_eq!(verdict.state, State::Impossible);
    assert!(parse_verdict("{\"verdict\": \"maybe\", \"reason\": \"x\"}").is_err());

    let verdict = parse_verdict("{\"done\": true, \"reason\": \"tests pass\"}")
        .expect("valid legacy verdict");
    assert_eq!(verdict.state, State::Done);
    let verdict = parse_verdict("{\"done\": false, \"reason\": \"two files remain\"}")
        .expect("valid legacy verdict");
    assert_eq!(verdict.state, State::Continue);
}

#[test]
fn verdict_tolerates_prose_but_rejects_garbage() {
    let verdict =
        parse_verdict("Sure! {\"verdict\": \"done\", \"reason\": \"built\"} hope that helps")
            .expect("embedded verdict");
    assert_eq!(verdict.state, State::Done);
    assert_eq!(verdict.reason, "built");

    assert!(parse_verdict("the objective looks incomplete").is_err());
    assert!(parse_verdict("{\"done\": \"yes\"}").is_err());
    assert!(parse_verdict("done :) } still thinking {").is_err());
}

#[test]
fn opening_prompt_names_the_gates() {
    let text = prompt("fix the flaky tests", &["cargo test".to_string()], 0);
    assert!(text.contains("fix the flaky tests"));
    assert!(text.contains("`cargo test`"));
    let text = prompt("fix the flaky tests", &[], 0);
    assert!(!text.contains("check"));
}

#[test]
fn continuation_prompt_uses_the_active_work() {
    let text = prompt("ignored", &[], 1);
    assert_eq!(text, "[summon goal: continue the active Work]");
    assert!(text.starts_with("[summon goal:"));
}

#[test]
fn judge_input_includes_harness_evidence() {
    let input = judge_user(
        "fix the crash",
        "Done.",
        "Turn receipt\n- Changed: `src/main.rs`\n- Verification: `cargo test` passed",
    );

    assert!(input.contains("Objective:\nfix the crash"));
    assert!(input.contains("Harness evidence:\nTurn receipt"));
    assert!(input.contains("`cargo test` passed"));
    assert!(input.contains("Agent's latest response:\nDone."));
}
