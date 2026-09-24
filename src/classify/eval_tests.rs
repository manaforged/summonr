use super::*;

fn noul(probability: f64) -> Answer {
    Answer::Noul { noul: probability }
}

fn choice(pick: &str, confidence: f64) -> Answer {
    Answer::Choice {
        choice: pick.to_string(),
        probabilities: BTreeMap::from([(pick.to_string(), confidence)]),
        confidence,
    }
}

fn decided(answers: &[(&str, Answer)]) -> Decision {
    Decision {
        model: "m".to_string(),
        answers: answers
            .iter()
            .map(|(name, answer)| ((*name).to_string(), answer.clone()))
            .collect(),
        usage: super::super::Usage::default(),
    }
}

fn example(name: &str, expect: &[(&str, serde_json::Value)]) -> Example {
    Example {
        state: serde_json::Value::from("s"),
        name: Some(name.to_string()),
        expect: expect
            .iter()
            .map(|(key, value)| ((*key).to_string(), value.clone()))
            .collect(),
    }
}

#[test]
fn a_confident_no_counts_as_certain_not_as_doubt() {
    let verdict = judge(&noul(0.04), &serde_json::json!(false)).unwrap();
    assert!(verdict.right);
    assert!(
        (verdict.certainty - 0.96).abs() < 1e-9,
        "p=0.04 for a false claim is a confident no, got {}",
        verdict.certainty
    );
}

#[test]
fn a_noul_on_the_wrong_side_of_the_line_is_a_miss() {
    let verdict = judge(&noul(0.77), &serde_json::json!(false)).unwrap();
    assert!(!verdict.right, "0.77 means yes, and the truth was no");
    assert_eq!(verdict.got, "true");
}

#[test]
fn a_score_within_half_a_level_counts_as_right() {
    let near = Answer::Score {
        score: 2.3,
        legend: BTreeMap::new(),
        probabilities: BTreeMap::new(),
        confidence: 0.8,
    };
    assert!(judge(&near, &serde_json::json!(2.0)).unwrap().right);
    let far = Answer::Score {
        score: 1.2,
        legend: BTreeMap::new(),
        probabilities: BTreeMap::new(),
        confidence: 0.8,
    };
    assert!(!judge(&far, &serde_json::json!(2.0)).unwrap().right);
}

#[test]
fn the_gate_is_what_tells_you_the_set_is_usable() {
    let examples = vec![
        example("akamai", &[("kind", serde_json::json!("antibot"))]),
        example("tls", &[("kind", serde_json::json!("network"))]),
        example("oos", &[("kind", serde_json::json!("stock"))]),
    ];
    let decided = vec![
        decided(&[("kind", choice("auth", 0.60))]),
        decided(&[("kind", choice("network", 1.0))]),
        decided(&[("kind", choice("stock", 1.0))]),
    ];
    let (tallies, misses) = score(&examples, &decided, 0.7).unwrap();
    let kind = &tallies["kind"];
    assert_eq!(kind.asked, 3);
    assert_eq!(kind.right, 2);
    assert_eq!(
        kind.acted, 2,
        "the 0.60 answer sits below the gate, so it is never acted on"
    );
    assert_eq!(
        kind.acted_right, 2,
        "everything above the gate was correct, which is the number that matters"
    );
    assert!((kind.gated_accuracy() - 1.0).abs() < 1e-9);
    assert_eq!(misses.len(), 1);
    assert_eq!(misses[0].example, "akamai");
    assert_eq!(misses[0].got, "auth");
}

#[test]
fn a_question_the_labels_do_not_cover_is_not_scored() {
    let examples = vec![example("one", &[("kind", serde_json::json!("stock"))])];
    let decided = vec![decided(&[
        ("kind", choice("stock", 0.9)),
        ("rotate", noul(0.8)),
    ])];
    let (tallies, _) = score(&examples, &decided, 0.7).unwrap();
    assert!(
        !tallies.contains_key("rotate"),
        "an unlabelled question must not be counted as right"
    );
}

#[test]
fn examples_load_one_per_line_and_report_the_bad_one() {
    let examples = load(
        "{\"state\":\"a\",\"expect\":{\"kind\":\"stock\"}}\n\n{\"state\":\"b\",\"expect\":{\"kind\":\"auth\"}}\n",
    )
    .unwrap();
    assert_eq!(examples.len(), 2);
    let error = load("{\"state\":\"a\"}\n").unwrap_err().to_string();
    assert!(error.contains("line 1"), "got {error}");
    load("").unwrap_err();
}

#[test]
fn the_report_names_the_miss_so_you_can_fix_the_criteria() {
    let examples = vec![example("akamai", &[("kind", serde_json::json!("antibot"))])];
    let decided = vec![decided(&[("kind", choice("auth", 0.6))])];
    let (tallies, misses) = score(&examples, &decided, 0.7).unwrap();
    let text = report(&tallies, &misses, 0.7);
    assert!(text.contains("kind"));
    assert!(text.contains("akamai"), "got {text}");
    assert!(text.contains("antibot"), "got {text}");
    assert!(text.contains("auth"), "got {text}");
}
