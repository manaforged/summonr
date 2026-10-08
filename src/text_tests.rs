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

#[test]
fn a_provider_error_never_echoes_a_key_fragment() {
    let message = "Incorrect API key provided: sk-ab12*****xy9z. You can find your API key at https://example.com.";
    let shown = hide_key_echo(message);
    assert_eq!(
        shown,
        "Incorrect API key provided: [key]. You can find your API key at https://example.com."
    );
    assert_eq!(
        hide_key_echo("rate limited, retry later"),
        "rate limited, retry later"
    );
}
