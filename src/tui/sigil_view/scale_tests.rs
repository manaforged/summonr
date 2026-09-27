use sigil::Host;
use sigil::app::cell_width;
use sigil::events::EventRouter;
use sigil::prelude::Cx;
use sigil::reactive::Runtime;
use sigil::terminal::Screen;
use tokio::sync::mpsc::unbounded_channel;

use super::app::App;
use super::profile::info;
use super::view::root;
use crate::tools::Event;
use crate::tui::{Activity, Info, Msg};

mod effort;
mod gutter;
mod mention;
mod picker;
mod secret;

const SIZES: [(u16, u16); 5] = [(40, 12), (60, 20), (80, 24), (110, 34), (200, 60)];

#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    Idle,
    Busy,
    Tool,
}

fn frame(cols: u16, rows: u16, state: State) -> Vec<String> {
    frame_with(busy_info(), cols, rows, state, |_| {})
}

fn busy_info() -> Info {
    Info {
        model: "stub-model".to_owned(),
        pick: Some("flag"),
        relay: Some("127.0.0.1:18080".to_owned()),
        session: "0123456789abcdef".to_owned(),
        ..info()
    }
}

fn frame_with(
    base: Info,
    cols: u16,
    rows: u16,
    state: State,
    setup: impl FnOnce(&App),
) -> Vec<String> {
    Runtime::scope("summonr-scale", || {
        let workspace = tempfile::tempdir().expect("workspace");
        std::fs::create_dir(workspace.path().join(".git")).expect("git dir");
        std::fs::write(workspace.path().join(".git/HEAD"), "ref: refs/heads/main\n").expect("head");
        let (commands, _receiver) = unbounded_channel();
        let app = App::new(base, workspace.path().to_owned(), commands);
        app.resize(cols, rows);
        app.update(Msg::User("Review the change".to_owned()));
        if state == State::Tool {
            app.update(Msg::Event(Event::ToolCall {
                id: "t1".to_owned(),
                name: "read".to_owned(),
                arguments: r#"{"path":"src/main.rs"}"#.to_owned(),
            }));
            app.update(Msg::Event(Event::ToolResult {
                id: "t1".to_owned(),
                success: true,
                mutated: false,
                output: "fn main() {}".to_owned(),
                millis: Some(4),
            }));
        }
        app.update(Msg::Event(Event::Assistant {
            text: "Measured layout.".to_owned(),
        }));
        if state != State::Idle {
            app.update(Msg::Busy(Some(Activity::Turn)));
        }
        setup(&app);
        app.refresh();
        let mut host = Host::cells();
        let tree = app.clone();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut screen = Screen::new(cols, rows);
        let mut router = EventRouter::new();
        for _ in 0..2 {
            screen
                .frame(&mut host, &mut router, [], (cols, rows))
                .expect("frame");
        }
        let grid = screen
            .frame(&mut host, &mut router, [], (cols, rows))
            .expect("frame");
        grid.text().lines().map(str::to_owned).collect()
    })
}

fn find(lines: &[String], needle: &str) -> Option<usize> {
    lines.iter().position(|line| line.contains(needle))
}

fn composer_top(lines: &[String]) -> usize {
    find(lines, "● auto").unwrap_or_else(|| panic!("no composer header:\n{}", lines.join("\n")))
}

fn below_transcript(lines: &[String], label: &str) -> usize {
    let last = lines
        .iter()
        .rposition(|line| line.contains("Measured layout."))
        .unwrap_or_else(|| panic!("{label}: transcript pushed off:\n{}", lines.join("\n")));
    composer_top(lines).saturating_sub(last + 1)
}

#[test]
fn busy_row_takes_at_most_two_rows_at_every_size() {
    for (cols, rows) in SIZES {
        let idle = below_transcript(&frame(cols, rows, State::Idle), "idle");
        for state in [State::Busy, State::Tool] {
            let busy = frame(cols, rows, state);
            let label = format!("{cols}x{rows} {state:?}");
            let status = find(&busy, "thinking")
                .unwrap_or_else(|| panic!("{label}: no busy row:\n{}", busy.join("\n")));
            let taken = below_transcript(&busy, &label).saturating_sub(idle);
            assert!(
                taken <= 2 && status < composer_top(&busy),
                "{label}: busy row takes {taken} rows:\n{}",
                busy.join("\n")
            );
        }
    }
}

#[test]
fn composer_footer_and_branch_fit_every_size() {
    for (cols, rows) in SIZES {
        for state in [State::Idle, State::Busy, State::Tool] {
            let lines = frame(cols, rows, state);
            let shown = lines.join("\n");
            assert!(
                lines.len() <= usize::from(rows),
                "{cols}x{rows} {state:?}: {} rows",
                lines.len()
            );
            for line in &lines {
                assert!(
                    cell_width(line.trim_end()) <= usize::from(cols),
                    "{cols}x{rows} {state:?}: overflow {line:?}"
                );
            }
            header_readable(&lines, &format!("{cols}x{rows} {state:?}"));
            let top = composer_top(&lines);
            let footer = find(&lines, "⎇ main")
                .unwrap_or_else(|| panic!("{cols}x{rows} {state:?}: no branch:\n{shown}"));
            assert!(
                top < footer,
                "{cols}x{rows} {state:?}: footer above composer:\n{shown}"
            );
            assert!(
                !shown.contains('⌞'),
                "{cols}x{rows} {state:?}: corner glyph:\n{shown}"
            );
            assert!(
                find(&lines, "send")
                    .or_else(|| find(&lines, "cancel"))
                    .is_some(),
                "{cols}x{rows} {state:?}: composer hint cut:\n{shown}"
            );
        }
    }
}

#[test]
fn tool_card_shows_at_every_size() {
    for (cols, rows) in SIZES {
        let lines = frame(cols, rows, State::Tool);
        assert!(
            find(&lines, "read").is_some(),
            "{cols}x{rows}: no tool card:\n{}",
            lines.join("\n")
        );
    }
}

fn header_readable(lines: &[String], label: &str) {
    let line = lines.first().map_or("", String::as_str);
    let parts = [
        "summonr",
        "stub-model",
        "(flag)",
        "· 127.0.0.1:18080",
        "ses ",
        "auto",
    ];
    for part in parts {
        let head: String = part.chars().take(3).collect();
        let Some(start) = line.find(&head) else {
            continue;
        };
        let rest = &line[start..];
        let word = rest.split(' ').next().unwrap_or_default();
        let whole = rest
            .strip_prefix(part)
            .is_some_and(|tail| part.ends_with(' ') || tail.is_empty() || tail.starts_with(' '));
        assert!(
            whole || (part == "stub-model" && word.ends_with('…')),
            "{label}: segment {part:?} cut in {line:?}"
        );
        assert!(
            start == 0 || line[..start].ends_with(' '),
            "{label}: segment {part:?} not separated in {line:?}"
        );
    }
    assert!(line.contains("summonr "), "{label}: brand cut in {line:?}");
}

#[test]
fn header_is_readable_when_narrow() {
    for cols in [40, 50, 60] {
        for state in [State::Idle, State::Busy] {
            header_readable(&frame(cols, 15, state), &format!("{cols} cols {state:?}"));
        }
    }
}

#[test]
fn secret_prompt_renders_as_a_modal() {
    for (cols, rows) in SIZES {
        let lines = frame_with(busy_info(), cols, rows, State::Idle, |app| {
            app.set_secret(Some(("Password for git:".to_owned(), 3)));
        });
        let label = format!("{cols}x{rows} secret");
        assert!(
            find(&lines, "Credential").is_some(),
            "{label}: no modal:\n{}",
            lines.join("\n")
        );
        let at = find(&lines, "Password")
            .unwrap_or_else(|| panic!("{label}: no prompt:\n{}", lines.join("\n")));
        assert!(
            lines[at].contains('┃'),
            "{label}: prompt outside the dialog:\n{}",
            lines.join("\n")
        );
    }
}

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
fn retry_shows_one_live_thinking_row() {
    let shown = frame_with(busy_info(), 110, 40, State::Idle, |app| {
        app.update(Msg::Event(Event::Reasoning {
            text: "Plan the change.".to_owned(),
        }));
        app.update(Msg::Event(Event::Assistant {
            text: "Cancelled.".to_owned(),
        }));
        app.update(Msg::Event(Event::Reasoning {
            text: "Plan the change.".to_owned(),
        }));
        app.update(Msg::Busy(Some(Activity::Turn)));
    })
    .join("\n");
    assert_eq!(shown.matches("Thinking…").count(), 1, "{shown}");
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

#[test]
fn expanded_thinking_renders_in_full_live_and_after_the_turn() {
    let body = (1..=20)
        .map(|line| format!("step {line} of the plan.\n"))
        .collect::<String>();
    let setup = |busy: bool| {
        let body = body.clone();
        move |app: &App| {
            app.model_mut(|model| model.prefs.reasoning = crate::prefs::Visibility::Expanded);
            app.update(Msg::Event(Event::Reasoning { text: body }));
            if busy {
                app.update(Msg::Busy(Some(Activity::Turn)));
            }
        }
    };
    for busy in [true, false] {
        let frame = frame_with(busy_info(), 110, 40, State::Idle, setup(busy)).join("\n");
        assert!(
            frame.contains("step 1 of") && frame.contains("step 20 of"),
            "{frame}"
        );
    }
}
