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
        patch: None,
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
fn a_read_shows_its_range_on_one_line() {
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
    assert!(shown.contains("● Read src/report.py 1-10"), "{shown}");
    assert!(!shown.contains("10:row 10"), "{shown}");
}

#[test]
fn run_output_hangs_under_the_call_and_counts_lines() {
    let body = (1..=10)
        .map(|line| format!("row {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let shown = card(110, |app| {
        tool(
            app,
            "run_command",
            r#"{"command":"seq"}"#,
            true,
            &format!("exit 0\n{body}"),
        );
    });
    assert!(shown.contains("● Run seq"), "{shown}");
    assert!(shown.contains("⎿ row 1"), "{shown}");
    assert!(
        shown.contains("+7 lines") && shown.contains("row 10"),
        "{shown}"
    );
}

#[test]
fn a_failed_run_names_its_exit_code() {
    let shown = card(110, |app| {
        tool(
            app,
            "run_command",
            r#"{"command":"pytest -q"}"#,
            false,
            "exit 1\nFAILED test_calc.py::test_median\n1 failed, 2 passed",
        );
    });
    let line = shown
        .lines()
        .find(|line| line.contains("Run pytest -q"))
        .unwrap_or_default();
    assert!(line.contains("exit 1"), "{shown}");
    assert!(
        shown.contains("FAILED test_calc.py::test_median"),
        "{shown}"
    );
}

#[test]
fn consecutive_reads_collapse_into_one_explored_line() {
    let shown = card(110, |app| {
        for (id, path) in [("r1", "a.py"), ("r2", "b.py")] {
            app.update(Msg::Event(Event::ToolCall {
                id: id.to_owned(),
                name: "read".to_owned(),
                arguments: format!(r#"{{"path":"{path}"}}"#),
            }));
            app.update(Msg::Event(Event::ToolResult {
                id: id.to_owned(),
                success: true,
                mutated: false,
                output: format!("{path}  lines 1-10/10\n10:"),
                millis: Some(4),
                patch: None,
            }));
        }
    });
    assert!(shown.contains("● Explored 2 files"), "{shown}");
    assert!(shown.contains("⎿ Read a.py 1-10"), "{shown}");
    assert!(shown.contains("Read b.py 1-10"), "{shown}");
    assert!(
        !shown.lines().any(|line| line.trim_end().ends_with(" 10")),
        "{shown}"
    );
}

#[test]
fn an_edit_shows_its_diff_under_one_call_line() {
    let patch = "edited src/one.py\ndiff --git a/src/one.py b/src/one.py\n--- a/src/one.py\n+++ b/src/one.py\n@@ -1 +1 @@\n-first_old\n+first_new";
    let shown = card(160, |app| {
        tool(app, "edit", r#"{"path":"src/one.py"}"#, true, patch);
    });
    assert!(shown.contains("● Edit src/one.py"), "{shown}");
    assert!(
        shown.contains("first_old") && shown.contains("first_new"),
        "{shown}"
    );
    assert!(shown.contains("+1 −1"), "{shown}");
    assert!(
        !shown.contains("unified") && !shown.contains("Before") && !shown.contains("@@"),
        "{shown}"
    );
    assert!(!shown.contains("Applied"), "{shown}");
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
    assert!(shown.contains(r#"Search "value_cents" *.py"#), "{shown}");
    assert!(shown.contains("src/models.py:17:7"), "{shown}");
    assert!(shown.contains(&long), "{shown}");
}

#[test]
fn an_edit_result_draws_the_patch_it_carries() {
    let shown = card(160, |app| {
        app.update(Msg::Event(Event::ToolCall {
            id: "e1".to_owned(),
            name: "edit".to_owned(),
            arguments: r#"{"path":"calc.py"}"#.to_owned(),
        }));
        app.update(Msg::Event(Event::ToolResult {
            id: "e1".to_owned(),
            success: true,
            mutated: true,
            output: "edited calc.py\nchanged line 2".to_owned(),
            millis: Some(4),
            patch: Some(
                "@@ -1,2 +1,2 @@\n def mean(values):\n-    return total + 1\n+    return total"
                    .to_owned(),
            ),
        }));
    });
    assert!(shown.contains("● Edit calc.py"), "{shown}");
    assert!(shown.contains("return total + 1"), "{shown}");
    assert!(!shown.contains("changed line 2"), "{shown}");
}

#[test]
fn a_read_of_several_files_names_each_one() {
    let shown = card(110, |app| {
        tool(
            app,
            "read",
            r#"{"paths":["calc.py","test_calc.py"]}"#,
            true,
            "calc.py  lines 1-7/7\n1:def mean(values):",
        );
    });
    let line = shown
        .lines()
        .find(|line| line.contains("● Read"))
        .unwrap_or_default();
    assert!(line.contains("calc.py, test_calc.py"), "{shown}");
    assert!(!line.contains("1-7"), "{shown}");
}

#[test]
fn a_default_read_mode_stays_off_the_call_line() {
    let shown = card(110, |app| {
        tool(
            app,
            "read",
            r#"{"path":"calc.py","op":"file"}"#,
            true,
            "calc.py  lines 1-7/7\n1:def mean(values):",
        );
    });
    assert!(shown.contains("● Read calc.py 1-7"), "{shown}");
}

#[test]
fn the_fold_key_cycles_thinking_and_tool_output() {
    let mut seen = Vec::new();
    let _ = card(110, |app| {
        for _ in 0..3 {
            assert!(app.route_hotkey('n'));
            let prefs = app.model.peek().prefs.clone();
            seen.push((prefs.reasoning.name(), prefs.tools.name()));
        }
    });
    assert_eq!(
        seen,
        [
            ("collapsed", "hidden"),
            ("expanded", "compact"),
            ("collapsed", "compact"),
        ]
    );
}

#[test]
fn hidden_tool_output_keeps_only_the_call_line() {
    let shown = card(110, |app| {
        app.set_tools(crate::prefs::ToolOutput::Hidden);
        tool(
            app,
            "run_command",
            r#"{"command":"seq 3"}"#,
            true,
            "exit 0\nrow one\nrow two",
        );
    });
    assert!(shown.contains("● Run seq 3"), "{shown}");
    assert!(!shown.contains("row two"), "{shown}");
}

#[test]
fn tool_lines_show_workspace_paths_relative() {
    let lead = std::cell::Cell::new(' ');
    let shown = card(110, |app| {
        let inside = app.workspace.join("src/calc.py").display().to_string();
        let arguments = serde_json::json!({ "path": inside }).to_string();
        tool(
            app,
            "read",
            &arguments,
            true,
            "src/calc.py  lines 1-3/3\n1:a",
        );
        tool(
            app,
            "run_command",
            r#"{"command":"cat /etc/hosts"}"#,
            true,
            "exit 0\nok",
        );
        let root = app.workspace.display().to_string();
        lead.set(root.chars().next().unwrap_or(' '));
        let sep = std::path::MAIN_SEPARATOR;
        let copy = format!("cp {root}{sep}x /backup{root}{sep}x");
        let arguments = serde_json::json!({ "command": copy }).to_string();
        tool(app, "run_command", &arguments, true, "exit 0");
    });
    assert!(shown.contains("● Read src/calc.py"), "{shown}");
    assert!(shown.contains("cat /etc/hosts"), "{shown}");
    assert!(
        shown.contains(&format!("Run cp x /backup{}", lead.get())),
        "{shown}"
    );
}

#[test]
fn a_search_jump_opens_that_tool_and_keeps_the_saved_setting() {
    let mut state = None;
    let _ = card(110, |app| {
        app.set_tools(crate::prefs::ToolOutput::Hidden);
        tool(
            app,
            "run_command",
            r#"{"command":"seq 3"}"#,
            true,
            "exit 0\nneedle row",
        );
        app.search.start("needle");
        app.jump();
        let model = app.model.peek();
        let opened = model.entries.iter().any(|entry| {
            matches!(
                entry,
                crate::tui::sigil_view::model::Entry::Tool { expanded: true, .. }
            )
        });
        state = Some((opened, model.prefs.tools));
    });
    assert_eq!(state, Some((true, crate::prefs::ToolOutput::Hidden)));
}
