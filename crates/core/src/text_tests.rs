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
fn frontmatter_reads_block_scalars_with_each_ending() {
    for (marker, expected) in [
        (">", "one two\n"),
        (">-", "one two"),
        (">+", "one two\n\n"),
        ("|", "one\ntwo\n"),
        ("|-", "one\ntwo"),
        ("|+", "one\ntwo\n\n"),
    ] {
        let text = format!("---\ndescription: {marker}\n  one\n  two\n\nname: next\n---\nBody.\n");
        let front = Frontmatter::parse(&text);
        assert_eq!(front.get("description"), Some(expected), "{marker}");
        assert_eq!(front.get("name"), Some("next"));
        assert_eq!(front.body, "Body.\n");
    }
}

#[test]
fn literal_metadata_keeps_indentation_and_does_not_create_fields() {
    let front = Frontmatter::parse(
        "---\ndescription: |-\n  first\n    indented\n  notes: still text\n  - still text\n  ---\ntools:\n  - Read\n  - Grep\n---\nBody.\n",
    );
    assert_eq!(
        front.get("description"),
        Some("first\n  indented\nnotes: still text\n- still text\n---")
    );
    assert_eq!(front.get("notes"), None);
    assert_eq!(
        front.list("tools"),
        Some(vec!["Read".to_owned(), "Grep".to_owned()])
    );
    assert_eq!(front.body, "Body.\n");
}

#[test]
fn folded_metadata_keeps_paragraphs_and_more_indented_lines() {
    let front = Frontmatter::parse(
        "---\ndescription: >-\n  first\n  second\n\n  paragraph\n    code\n\n  last\n---\nBody.\n",
    );
    assert_eq!(
        front.get("description"),
        Some("first second\nparagraph\n  code\n\nlast")
    );
}

#[test]
fn quoted_block_markers_remain_scalar_text() {
    let front = Frontmatter::parse("---\nfolded: '>'\nliteral: \"|-\"\n---\nBody.\n");
    assert_eq!(front.get("folded"), Some(">"));
    assert_eq!(front.get("literal"), Some("|-"));
    assert_eq!(front.body, "Body.\n");
}

#[test]
fn block_metadata_preserves_crlf_body_and_unicode_text() {
    let front = Frontmatter::parse(
        "---\r\ndescription: >-\r\n  análise: cuidado\r\n  λ\r\n---\r\nBody.\r\n",
    );
    assert_eq!(front.get("description"), Some("análise: cuidado λ"));
    assert_eq!(front.body, "Body.\r\n");
}

#[test]
fn unclosed_block_metadata_keeps_the_original_body() {
    for text in [
        "---\ndescription: >\n  unclosed\n",
        "---\ndescription: |-\n  unclosed\n",
    ] {
        let front = Frontmatter::parse(text);
        assert_eq!(front.get("description"), None);
        assert_eq!(front.body, text);
    }
}

#[test]
fn malformed_block_metadata_keeps_other_fields_and_the_body() {
    for text in [
        "---\ntools: [Read]\ndescription: |\n    first\n  invalid indent\n---\nBody.\n",
        "---\ndescription: |\n    first\n  invalid indent\ntools: [Read]\n---\nBody.\n",
    ] {
        let front = Frontmatter::parse(text);
        assert_eq!(front.get("description"), Some("|"));
        assert_eq!(front.list("tools"), Some(vec!["Read".to_owned()]));
        assert_eq!(front.body, "Body.\n");
    }
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
