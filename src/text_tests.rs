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

#[test]
fn frontmatter_reads_fields_and_leaves_the_body() {
    let front = Frontmatter::parse(
        "---\nname: \"reviewer\"\ntools: Read, Grep\nempty:\n---\nReview the diff.\n",
    );
    assert_eq!(front.get("name"), Some("reviewer"));
    assert_eq!(front.get("tools"), Some("Read, Grep"));
    assert_eq!(front.get("empty"), None);
    assert_eq!(front.body, "Review the diff.\n");
}

#[test]
fn text_without_a_closed_fence_is_all_body() {
    for text in ["# Title\nbody", "---\nname: x\nno closing fence"] {
        let front = Frontmatter::parse(text);
        assert_eq!(front.get("name"), None);
        assert_eq!(front.body, text);
    }
}

#[test]
fn a_frontmatter_list_reads_every_form_yaml_allows() {
    let comma = Frontmatter::parse("---\ntools: Read, Grep\n---\n");
    let inline = Frontmatter::parse("---\ntools: [Read, Grep]\n---\n");
    let block = Frontmatter::parse("---\ntools:\n  - Read\n  - Grep\n---\n");
    for front in [&comma, &inline, &block] {
        assert_eq!(
            front.list("tools"),
            Some(vec!["Read".to_string(), "Grep".to_string()])
        );
    }
    assert_eq!(
        Frontmatter::parse("---\ntools: [\"Grep, Glob\", Read]\n---\n").list("tools"),
        Some(vec!["Grep, Glob".to_string(), "Read".to_string()])
    );
    assert_eq!(
        Frontmatter::parse("---\ntools:\n---\n").list("tools"),
        Some(Vec::new())
    );
    assert_eq!(
        Frontmatter::parse("---\nname: x\n---\n").list("tools"),
        None
    );
    assert_eq!(
        Frontmatter::parse("---\n\"tools\": [Read]\n---\n").list("tools"),
        Some(vec!["Read".to_string()])
    );
}
