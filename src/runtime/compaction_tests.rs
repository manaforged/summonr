use super::*;

#[test]
fn the_kept_tail_never_starts_before_the_turn_opener() {
    let message = |role: Role| Message {
        role,
        ..Message::system("0123456789")
    };
    let messages = [
        Message::system("0123456789"),
        message(Role::User),
        message(Role::Assistant),
        message(Role::User),
        message(Role::Assistant),
        message(Role::Assistant),
    ];
    assert_eq!(compact_boundary(&messages, 40, false, Some(2)), Some(4));
}
