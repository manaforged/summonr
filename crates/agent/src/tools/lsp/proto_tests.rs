use serde_json::json;

use super::{goal, has_error};

#[test]
fn lean_goals_keep_each_proof_state() -> Result<(), Box<dyn std::error::Error>> {
    let value = json!({"rendered": "ignored", "goals": ["x : Nat\n⊢ x = x", "⊢ True"]});
    assert_eq!(goal(value)?, "x : Nat\n⊢ x = x\n\n⊢ True");
    assert_eq!(
        goal(json!({"rendered": "no goals", "goals": []}))?,
        "no goals"
    );
    assert_eq!(goal(json!(null))?, "no tactic goal at this position");
    goal(json!({"rendered": "broken", "goals": "not a list"})).unwrap_err();
    Ok(())
}

#[test]
fn an_error_after_a_warning_still_counts_as_an_error() {
    let report = json!({"items": [
        {"severity": 2, "message": "unused variable"},
        {"severity": 1, "message": "mismatched types"},
    ]});
    assert!(has_error(&report));
    assert!(!has_error(
        &json!({"items": [{"severity": 2, "message": "unused"}]})
    ));
}
