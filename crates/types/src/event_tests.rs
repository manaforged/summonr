use super::*;

#[test]
fn a_step_event_prints_the_documented_line() {
    let line = serde_json::to_string(&Event::Step {
        index: 2,
        total: 3,
        state: StepState::CheckFailed,
        check: None,
    })
    .unwrap();
    assert_eq!(
        line,
        r#"{"type":"step","index":2,"total":3,"state":"check failed","check":null}"#
    );
}
