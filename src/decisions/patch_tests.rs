use super::*;

const PATCH: &str = "diff --git a/src/one.rs b/src/one.rs\nindex 111..222 100644\n--- a/src/one.rs\n+++ b/src/one.rs\n@@ -1 +1 @@\n-let a = 1;\n+let a = 2;\ndiff --git a/src/two.rs b/src/two.rs\nindex 333..444 100644\n--- a/src/two.rs\n+++ b/src/two.rs\n@@ -1 +1 @@\n-ok()\n+unwrap()\n";

fn scored(risk: f64, kind: &str, nouls: &[(&str, f64)]) -> Decision {
    let mut answers = BTreeMap::from([
        (
            "risk".to_string(),
            Answer::Score {
                score: risk,
                legend: BTreeMap::new(),
                probabilities: BTreeMap::new(),
                confidence: 0.9,
            },
        ),
        (
            "kind".to_string(),
            Answer::Choice {
                choice: kind.to_string(),
                probabilities: BTreeMap::new(),
                confidence: 0.9,
            },
        ),
    ]);
    for (name, probability) in nouls {
        answers.insert((*name).to_string(), Answer::Noul { noul: *probability });
    }
    Decision {
        model: "m".to_string(),
        answers,
        usage: super::super::Usage::default(),
    }
}

#[test]
fn a_patch_becomes_one_state_per_file() {
    let changes = split(PATCH);
    assert_eq!(changes.len(), 2);
    assert_eq!(changes[0].path, "src/one.rs");
    assert_eq!(changes[1].path, "src/two.rs");
    assert!(
        changes[1].text.contains("unwrap()"),
        "each file carries its own hunks"
    );
    assert!(
        !changes[0].text.contains("unwrap()"),
        "one file's lines must not leak into another's state"
    );
}

#[test]
fn every_line_of_the_patch_lands_in_some_file() {
    let changes = split(PATCH);
    let kept: usize = changes
        .iter()
        .map(|change| change.text.lines().count())
        .sum();
    assert_eq!(kept, PATCH.lines().count());
}

#[test]
fn a_patch_with_no_header_yields_nothing_rather_than_a_bogus_file() {
    assert!(split("just some text\n").is_empty());
}

#[test]
fn the_riskiest_file_is_listed_first() {
    let changes = split(PATCH);
    let decided = vec![
        scored(0.2, "chore", &[]),
        scored(2.9, "fix", &[("adds_panic", 0.95)]),
    ];
    let text = table(&changes, &decided, 0.7);
    let first = text.lines().next().unwrap();
    assert!(
        first.contains("src/two.rs"),
        "the reviewer reads the dangerous file first, got {first}"
    );
}

#[test]
fn only_flags_above_the_gate_reach_the_reviewer() {
    let changes = split(PATCH);
    let decided = vec![
        scored(1.0, "fix", &[("adds_panic", 0.95), ("untested", 0.30)]),
        scored(0.5, "chore", &[]),
    ];
    let text = table(&changes, &decided, 0.7);
    assert!(text.contains("adds_panic 0.95"), "got {text}");
    assert!(
        !text.contains("untested"),
        "a 0.30 answer is the model declining, not a finding: {text}"
    );
}

#[test]
fn a_long_path_keeps_its_end_where_the_filename_is() {
    let long = "crates/modules/target/src/checkout/steps/place/inner/deep.rs";
    let clipped = clip(long, 44);
    assert_eq!(clipped.chars().count(), 44);
    assert!(clipped.ends_with("deep.rs"), "got {clipped}");
}
