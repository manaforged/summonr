use super::*;

fn choice(pick: &str, probabilities: &[(&str, f64)], confidence: f64) -> Answer {
    Answer::Choice {
        choice: pick.to_string(),
        probabilities: probabilities
            .iter()
            .map(|(option, share)| ((*option).to_string(), *share))
            .collect(),
        confidence,
    }
}

fn decision(answers: &[(&str, Answer)]) -> Decision {
    Decision {
        model: "m".to_string(),
        answers: answers
            .iter()
            .map(|(name, answer)| ((*name).to_string(), answer.clone()))
            .collect(),
        usage: Usage {
            input_tokens: 10,
            output_tokens: 2,
            cost: Some(0.5),
        },
    }
}

#[test]
fn a_transcript_too_big_for_the_context_is_split_not_truncated() {
    let state = "line one\nline two\nline three\nline four\n".repeat(200);
    let parts = windows(&state, 400);
    assert!(parts.len() > 1, "expected several windows");
    assert_eq!(
        parts.concat(),
        state,
        "every character must survive: a review that drops the middle is a lie"
    );
    assert!(
        parts.iter().all(|part| part.chars().count() <= 400),
        "a window blew the budget"
    );
}

#[test]
fn a_state_that_fits_stays_one_window() {
    let parts = windows("short", 400);
    assert_eq!(parts, vec!["short".to_string()]);
}

#[test]
fn a_single_line_longer_than_the_budget_is_still_split() {
    let state = "x".repeat(1000);
    let parts = windows(&state, 300);
    assert_eq!(parts.concat(), state);
    assert!(parts.iter().all(|part| part.chars().count() <= 300));
}

#[test]
fn one_window_merges_to_itself() {
    let only = decision(&[("q", Answer::Noul { noul: 0.4 })]);
    let merged = merge(std::slice::from_ref(&only)).unwrap();
    assert_eq!(merged, only);
}

#[test]
fn a_failure_seen_in_one_window_survives_the_merge() {
    let quiet = decision(&[("looped", Answer::Noul { noul: 0.02 })]);
    let caught = decision(&[("looped", Answer::Noul { noul: 0.91 })]);
    let merged = merge(&[quiet.clone(), caught, quiet]).unwrap();
    let Some(Answer::Noul { noul }) = merged.answers.get("looped") else {
        panic!("looped went missing");
    };
    let seen = *noul;
    assert_eq!(
        seen, 0.91,
        "one window seeing the loop is the finding; averaging it away hides it"
    );
}

#[test]
fn the_worst_risk_across_windows_is_the_risk() {
    let mild = decision(&[(
        "risk",
        Answer::Score {
            score: 0.2,
            legend: BTreeMap::new(),
            probabilities: BTreeMap::new(),
            confidence: 0.9,
        },
    )]);
    let bad = decision(&[(
        "risk",
        Answer::Score {
            score: 2.8,
            legend: BTreeMap::new(),
            probabilities: BTreeMap::new(),
            confidence: 0.7,
        },
    )]);
    let merged = merge(&[mild.clone(), bad, mild]).unwrap();
    let Some(Answer::Score { score, .. }) = merged.answers.get("risk") else {
        panic!("risk went missing");
    };
    let worst = *score;
    assert_eq!(worst, 2.8, "a risky window is the risk, not the average");
}

#[test]
fn a_confident_window_outweighs_an_unsure_one() {
    let unsure = decision(&[(
        "outcome",
        choice("partial", &[("partial", 0.6), ("finished", 0.4)], 0.05),
    )]);
    let sure = decision(&[(
        "outcome",
        choice("finished", &[("partial", 0.1), ("finished", 0.9)], 0.95),
    )]);
    let merged = merge(&[unsure, sure]).unwrap();
    let Some(Answer::Choice {
        choice,
        probabilities,
        confidence,
    }) = merged.answers.get("outcome")
    else {
        panic!("outcome went missing");
    };
    assert_eq!(choice, "finished", "the unsure window must not win");
    let best = *confidence;
    assert_eq!(best, 0.95);
    let total: f64 = probabilities.values().sum();
    assert!(
        (total - 1.0).abs() < 1e-9,
        "the blended distribution must still sum to one, got {total}"
    );
}

#[test]
fn merging_adds_up_what_the_sweep_cost() {
    let one = decision(&[("q", Answer::Noul { noul: 0.1 })]);
    let merged = merge(&[one.clone(), one.clone(), one]).unwrap();
    assert_eq!(merged.usage.input_tokens, 30);
    assert_eq!(merged.usage.output_tokens, 6);
    assert_eq!(merged.usage.cost, Some(1.5));
}

#[test]
fn merging_nothing_is_an_error_not_an_empty_answer() {
    merge(&[]).unwrap_err();
}
