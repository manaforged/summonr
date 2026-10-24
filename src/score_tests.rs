use super::*;

fn config() -> ScoreConfig {
    ScoreConfig {
        command: Some("true".to_string()),
        targets: [("s".to_string(), 2.0)].into_iter().collect(),
        spend: None,
        budget: None,
        halt: None,
    }
}

fn entry(value: f64, changes: &[&str]) -> Entry {
    Entry {
        time: "2026-10-24T12:00:00Z".to_string(),
        session: None,
        head: Some("0123456789abcdef".to_string()),
        changes: changes.iter().map(|path| path.to_string()).collect(),
        scores: [("s".to_string(), value)].into_iter().collect(),
        error: None,
        spend: None,
        spend_start: None,
        halted: false,
    }
}

#[test]
fn the_board_keeps_the_best_score_and_names_a_stall_until_a_target_is_met() {
    let mut entries: Vec<Entry> = [1.0, 1.5, 1.2, 1.2, 1.1]
        .into_iter()
        .map(|value| entry(value, &[]))
        .collect();
    let board = Board::new(&config(), PathBuf::from("score.jsonl"), &entries);
    assert_eq!(board.rows[0].best, Some(1.5));
    assert_eq!(board.rows[0].current, Some(1.1));
    assert_eq!(board.unmet, vec!["s".to_string()]);
    assert_eq!(board.turns_since_best, 3);
    assert!(board.stalled);
    assert!(
        board
            .stall
            .as_deref()
            .unwrap()
            .ends_with("Change the mechanism, not the parameters."),
        "{:?}",
        board.stall
    );
    entries.push(entry(2.5, &[]));
    let board = Board::new(&config(), PathBuf::from("score.jsonl"), &entries);
    assert!(board.met);
    assert!(!board.stalled);
}

#[test]
fn the_board_text_rounds_values_and_shortens_the_changed_paths() {
    let paths: Vec<String> = (0..10).map(|n| format!("p{n}.rs")).collect();
    let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
    let board = Board::new(
        &config(),
        PathBuf::from("score.jsonl"),
        &[entry(1.149_200_000_000_000_2, &paths)],
    );
    let text = board.text();
    assert!(text.contains("s\t1.1492\t1.1492\t2\t0.8508\tno"), "{text}");
    assert!(
        text.contains("(changed: p0.rs, p1.rs, p2.rs, p3.rs, p4.rs, p5.rs, … 4 more)"),
        "{text}"
    );
}
