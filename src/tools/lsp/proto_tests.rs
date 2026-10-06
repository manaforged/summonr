use serde_json::json;

use super::has_error;

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
