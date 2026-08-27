use super::{parse, user};

#[test]
fn json_verdicts_parse_with_or_without_prose() {
    let pass = parse("Looks right.\n{\"pass\": true, \"findings\": \"\"}");
    assert!(pass.pass);
    assert_eq!(pass.findings, "");
    let fail = parse("{\"pass\": false, \"findings\": \"rename x\"}");
    assert!(!fail.pass);
    assert_eq!(fail.findings, "rename x");
    let unsure = parse("{\"findings\": \"maybe\"}");
    assert!(!unsure.pass, "a missing pass is not a pass");
}

#[test]
fn prose_without_json_is_a_rejection_carrying_the_text() {
    let verdict = parse("  I would not merge this: the loop never terminates.  ");
    assert!(!verdict.pass);
    assert_eq!(
        verdict.findings,
        "I would not merge this: the loop never terminates."
    );
    let broken = parse("{not json}");
    assert!(!broken.pass);
    assert_eq!(broken.findings, "{not json}");

    // A closing brace before an opening one must not panic on the slice.
    let reversed = parse("nope } more to do {");
    assert!(!reversed.pass);
    assert_eq!(reversed.findings, "nope } more to do {");
}

#[test]
fn the_reviewer_sees_objective_evidence_paths_and_the_tail() {
    let text = user(
        "add a flag",
        "done",
        "Turn receipt\n- Changed: `a.rs`",
        &["a.rs".to_string(), "b.rs".to_string()],
    );
    assert!(text.starts_with("Objective:\nadd a flag\n"));
    assert!(text.contains("Harness evidence:\nTurn receipt"));
    assert!(text.contains("Changed paths:\n- a.rs\n- b.rs\n"));
    assert!(text.ends_with("Agent's latest response:\ndone"));
    assert!(user("x", "y", "z", &[]).contains("Changed paths:\nnone"));
}
