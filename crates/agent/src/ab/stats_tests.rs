use super::{Lift, Report, Run, order};

fn run(set: &str, round: usize, passed: bool, tokens: u64) -> Run {
    Run {
        set: set.to_owned(),
        round,
        code: Some(if passed { 0 } else { 2 }),
        passed,
        tokens,
        ..Run::default()
    }
}

fn sets(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn runs_alternate_a_b_then_b_a() {
    let names: Vec<(usize, String)> = order(&sets(&["lean", "none"]), 3);
    let shown: Vec<(usize, &str)> = names
        .iter()
        .map(|(round, name)| (*round, name.as_str()))
        .collect();
    assert_eq!(
        shown,
        [
            (0, "lean"),
            (0, "none"),
            (1, "none"),
            (1, "lean"),
            (2, "lean"),
            (2, "none")
        ]
    );
}

#[test]
fn a_set_that_always_passes_where_none_always_fails_shows_lift() {
    let mut runs = Vec::new();
    for round in 0..6 {
        runs.push(run("lean", round, true, 1_200));
        runs.push(run("none", round, false, 1_000));
    }
    let report = Report::build(&sets(&["lean", "none"]), runs);
    let lean = &report.sets[0];
    assert_eq!((lean.runs, lean.passed, lean.pass_all), (6, 6, true));
    assert!(!report.sets[1].pass_all);
    let pair = &report.pairs[0];
    assert_eq!((pair.set.as_str(), pair.against.as_str()), ("lean", "none"));
    assert_eq!(pair.lift, Lift::Better);
    assert_eq!(
        (pair.pass.diff, pair.pass.low, pair.pass.high),
        (1.0, 1.0, 1.0)
    );
    assert_eq!(pair.tokens.diff, 200.0);
}

#[test]
fn a_split_result_reports_no_measured_lift() {
    let mut runs = Vec::new();
    for round in 0..6 {
        runs.push(run("lean", round, round % 2 == 0, 1_000));
        runs.push(run("full", round, round % 2 == 1, 1_000));
    }
    let report = Report::build(&sets(&["lean", "full"]), runs);
    let pair = &report.pairs[0];
    assert_eq!((pair.set.as_str(), pair.against.as_str()), ("full", "lean"));
    assert_eq!(pair.lift, Lift::NoMeasuredLift);
    assert!(pair.pass.low < 0.0 && pair.pass.high > 0.0, "{pair:?}");
}

#[test]
fn a_run_reads_its_tokens_cost_session_and_skills_from_its_events() {
    let log = [
        r#"{"type":"session","id":"abc"}"#,
        r#"{"type":"usage","prompt_tokens":100,"completion_tokens":20,"cost":0.5}"#,
        r#"{"type":"tool_call","id":"1","name":"skill","arguments":"{\"name\":\"review\"}"}"#,
        r#"{"type":"tool_call","id":"2","name":"skill","arguments":"{\"name\":\"review\"}"}"#,
        r#"{"type":"child","id":"c","event":{"type":"usage","prompt_tokens":5,"completion_tokens":5,"cost":0.25}}"#,
        r#"{"type":"child","id":"c","event":{"type":"session","id":"child"}}"#,
        "not json",
    ]
    .join("\n");
    let read = Run::read("lean", 0, Some(0), 3.0, &log);
    assert!(read.passed);
    assert_eq!(read.session.as_deref(), Some("abc"));
    assert_eq!(read.tokens, 130);
    assert_eq!(read.cost, Some(0.75));
    assert_eq!(read.skills, ["review"]);
}
