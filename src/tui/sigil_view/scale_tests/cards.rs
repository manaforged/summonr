use super::super::app::App;
use super::*;

fn tool(app: &App, name: &str, arguments: &str, success: bool, output: &str) {
    app.update(Msg::Event(Event::ToolCall {
        id: name.to_owned(),
        name: name.to_owned(),
        arguments: arguments.to_owned(),
    }));
    app.update(Msg::Event(Event::ToolResult {
        id: name.to_owned(),
        success,
        mutated: false,
        output: output.to_owned(),
        millis: Some(4),
    }));
}

fn card(cols: u16, setup: impl FnOnce(&App)) -> String {
    frame_with(busy_info(), cols, 40, State::Idle, setup).join("\n")
}

#[test]
fn failed_tool_card_shows_the_error() {
    let shown = card(110, |app| {
        tool(
            app,
            "edit",
            r#"{"path":"src/models.py"}"#,
            false,
            "tool error: old text not found in src/models.py\n  31: class Store:\n  32: def value_cents(self) -> int:",
        );
    });
    assert!(shown.contains("old text not found"), "{shown}");
    assert!(!shown.contains("32: def value_cents"), "{shown}");
}

#[test]
fn tool_output_drops_terminal_escapes() {
    let shown = card(110, |app| {
        tool(
            app,
            "run_command",
            r#"{"command":"pytest"}"#,
            true,
            "exit 0\n\u{1b}[33mwarning:\u{1b}[0m \u{1b}]8;;https://x.dev\u{7}link\u{1b}]8;;\u{7} done",
        );
    });
    assert!(shown.contains("warning: link done"), "{shown}");
    assert!(!shown.contains("[33m") && !shown.contains("8;;"), "{shown}");
}

#[test]
fn compact_preview_keeps_the_header_and_counts_lines() {
    let body = (1..=10)
        .map(|line| format!("{line}:row {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let shown = card(110, |app| {
        tool(
            app,
            "read",
            r#"{"path":"src/report.py"}"#,
            true,
            &format!("src/report.py  lines 1-10/10\n{body}"),
        );
    });
    assert!(shown.contains("src/report.py  lines 1-10/10"), "{shown}");
    assert!(
        shown.contains("+8 lines") && shown.contains("10:row 10"),
        "{shown}"
    );
}

#[test]
fn diff_card_shows_every_file() {
    let patch = "exit 0\ndiff --git a/one.py b/one.py\nindex 1..2 100644\n--- a/one.py\n+++ b/one.py\n@@ -1 +1 @@\n-first_old\n+first_new\ndiff --git a/two.py b/two.py\nindex 3..4 100644\n--- a/two.py\n+++ b/two.py\n@@ -1 +1 @@\n-second_old\n+second_new";
    let shown = card(160, |app| {
        tool(app, "run_command", r#"{"command":"git diff"}"#, true, patch);
    });
    assert!(
        shown.contains("first_new") && shown.contains("second_new"),
        "{shown}"
    );
    assert!(!shown.contains("Applied"), "{shown}");
}

#[test]
fn tool_headers_name_the_call() {
    let long = format!("echo {}", "x".repeat(90));
    let shown = card(200, |app| {
        tool(
            app,
            "read",
            r#"{"pattern":"value_cents","glob":"*.py"}"#,
            true,
            "a.py:1:x",
        );
        tool(
            app,
            "lsp",
            r#"{"path":"src/models.py","line":17,"character":7}"#,
            true,
            "def x",
        );
        tool(
            app,
            "run_command",
            &serde_json::json!({ "command": long }).to_string(),
            true,
            "ok",
        );
    });
    assert!(shown.contains(r#"search "value_cents" *.py"#), "{shown}");
    assert!(shown.contains("src/models.py:17:7"), "{shown}");
    assert!(shown.contains(&long), "{shown}");
}
