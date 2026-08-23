use super::*;

#[test]
fn markdown_keeps_heading_list_and_code_semantics() {
    let accent = rgb(105, 140, 210);
    let lines = markdown(
        "# Heading\n- item\n```rust\nlet value = \"ok\";\n// note\n```",
        40,
        accent,
    );

    assert_eq!(lines[0][0].text, "Heading");
    assert!(lines[0][0].bold);
    assert_eq!(lines[1][0].text, "• ");
    assert_eq!(lines[1][0].fg, accent);
    assert!(lines[2].iter().all(|span| span.bg == CODE_BG));
    assert!(lines[2].iter().any(|span| span.fg == CODE_STRING));
    assert!(lines[3].iter().any(|span| span.fg == CODE_COMMENT));
}
