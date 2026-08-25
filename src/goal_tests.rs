use super::*;

#[test]
fn verdict_parses_clean_json() {
    let verdict = parse_verdict("{\"done\": true, \"reason\": \"tests pass\"}");
    assert!(verdict.done);
    assert_eq!(verdict.reason, "tests pass");

    let verdict = parse_verdict("{\"done\": false, \"reason\": \"two files remain\"}");
    assert!(!verdict.done);
    assert_eq!(verdict.reason, "two files remain");
}

#[test]
fn verdict_tolerates_prose_and_garbage() {
    let verdict = parse_verdict("Sure! {\"done\": true, \"reason\": \"built\"} hope that helps");
    assert!(verdict.done);
    assert_eq!(verdict.reason, "built");

    // Unparsable replies fail open as "continue".
    let verdict = parse_verdict("the objective looks incomplete");
    assert!(!verdict.done);
    assert_eq!(verdict.reason, "the objective looks incomplete");
    let verdict = parse_verdict("{\"done\": \"yes\"}");
    assert!(!verdict.done);
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
