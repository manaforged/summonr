use super::*;

#[test]
fn markdown_keeps_heading_list_and_code_semantics() {
    let accent = rgb(105, 140, 210);
    let lines = markdown(
        "# Heading\n- item\n```rust\nlet value = \"ok\";\n// note\n```",
        40,
        accent,
    );

    assert_eq!(lines[0].spans[0].text, "Heading");
    assert!(lines[0].spans[0].bold);
    assert_eq!(lines[1].spans[0].text, "• ");
    assert_eq!(lines[1].spans[0].fg, accent);
    assert!(
        lines[2]
            .spans
            .iter()
            .all(|span| span.bg == palette().code_bg)
    );
    assert!(
        lines[2]
            .spans
            .iter()
            .any(|span| span.fg == palette().code_string)
    );
    assert!(
        lines[3]
            .spans
            .iter()
            .any(|span| span.fg == palette().code_comment)
    );
}

#[test]
fn thinking_renders_by_preference_on_a_panel() {
    let entry = Entry::Thinking("pondering\nmore".to_string());
    let lines = |reasoning| {
        entry_lines(
            &entry,
            40,
            rgb(105, 140, 210),
            reasoning,
            ToolOutput::Compact,
            Receipts::Off,
            false,
        )
    };

    assert!(lines(Reasoning::Off).is_empty());
    assert_eq!(lines(Reasoning::Collapsed).len(), 1);
    let expanded = lines(Reasoning::Expanded);
    assert_eq!(expanded.len(), 2);
    assert_eq!(expanded[0].bg, palette().thought_bg);
    assert_eq!(expanded[1].bg, palette().thought_bg);
    assert!(expanded[0].spans[0].italic);
    assert!(expanded[0].spans[0].text.starts_with("✻ "));
}

#[test]
fn receipts_render_by_preference() {
    let entry = Entry::Receipt("Changed: one\nFixed: two".to_string());
    let lines = |receipts| {
        entry_lines(
            &entry,
            40,
            rgb(105, 140, 210),
            Reasoning::Expanded,
            ToolOutput::Compact,
            receipts,
            false,
        )
    };

    assert!(lines(Receipts::Off).is_empty());
    assert_eq!(lines(Receipts::Collapsed).len(), 1);
    assert_eq!(lines(Receipts::Expanded).len(), 2);
}
