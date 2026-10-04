use super::*;

#[test]
fn durations_read_in_seconds_minutes_and_hours() {
    assert_eq!(duration(45), "45s");
    assert_eq!(duration(754), "12m34s");
    assert_eq!(duration(3723), "1h02m03s");
}

#[test]
fn counts_take_the_right_noun() {
    assert_eq!(plural(1, "hunk"), "1 hunk");
    assert_eq!(plural(3, "hunk"), "3 hunks");
    assert_eq!(plural(2, "match"), "2 matches");
    assert_eq!(plural(2, "child"), "2 children");
}
