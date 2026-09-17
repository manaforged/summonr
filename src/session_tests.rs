use super::*;

#[test]
fn user_text_strips_the_harness_note() {
    assert_eq!(
        user_text("hi\n\nHarness state: auto mode. Do only what they asked."),
        "hi"
    );
}
