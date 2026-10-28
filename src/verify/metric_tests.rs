use super::*;

fn metric(pattern: Option<&str>, better: Better, noise: f64, target: Option<f64>) -> Metric {
    Metric {
        command: "bench".to_string(),
        pattern: pattern.map(str::to_string),
        better,
        target,
        noise,
        timeout_secs: None,
        protect: Vec::new(),
    }
}

#[test]
fn the_value_is_the_pattern_group_or_else_the_last_number_printed() {
    let plain = metric(None, Better::Lower, 0.0, None);
    assert_eq!(
        plain.read("warmup 3 runs\nmean 2.5e1 us\n"),
        Some(("2.5e1".to_string(), 25.0))
    );
    assert_eq!(plain.read("no numbers here"), None);
    let grouped = metric(Some(r"geomean x([0-9.]+)"), Better::Higher, 0.0, None);
    assert_eq!(
        grouped.read("s1 x1.9\ngeomean x2.31\nshapes 12\n"),
        Some(("2.31".to_string(), 2.31))
    );
}

#[test]
fn a_value_counts_only_past_the_noise_in_the_better_direction_and_the_target_needs_a_best() {
    let lower = metric(None, Better::Lower, 1.0, Some(9.0));
    assert!(lower.beats(5.0, None));
    assert!(!lower.beats(9.5, Some(10.0)));
    assert!(lower.beats(8.9, Some(10.0)));
    assert!(lower.reached(Some(9.0)));
    assert!(!lower.reached(Some(9.1)));
    assert!(!lower.reached(None));
    let higher = metric(None, Better::Higher, 0.5, None);
    assert!(!higher.beats(10.4, Some(10.0)));
    assert!(higher.beats(10.6, Some(10.0)));
    assert!(!higher.reached(Some(1e9)));
}
