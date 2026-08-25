use super::route_checks;

#[test]
fn goal_owns_configured_checks() {
    let checks = vec!["cargo clippy".to_string(), "cargo test".to_string()];
    let (verify, goal) = route_checks(true, Some(checks.clone()));

    assert_eq!(verify, Some(checks.clone()));
    assert_eq!(goal, checks);
}

#[test]
fn ordinary_turns_keep_configured_checks() {
    let checks = vec!["cargo test".to_string()];
    let (verify, goal) = route_checks(false, Some(checks.clone()));

    assert_eq!(verify, Some(checks));
    assert!(goal.is_empty());
}
