use super::steerable;
use crate::provider::UserInput;

#[test]
fn steering_queue_holds_shell_and_slash_lines_for_their_own_turn() {
    for line in [
        "!git status",
        "!!cargo test",
        "/review the diff",
        "/github:pr 12",
    ] {
        assert!(!steerable(&UserInput::from(line)), "{line}");
    }
}

#[test]
fn steering_queue_sends_plain_messages_and_bang_inside_text() {
    for line in [
        "also check the tests",
        "  why did that fail?",
        "it says ! here",
    ] {
        assert!(steerable(&UserInput::from(line)), "{line}");
    }
}
