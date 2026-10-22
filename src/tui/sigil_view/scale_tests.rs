use sigil::Host;
use sigil::app::cell_width;
use sigil::events::EventRouter;
use sigil::reactive::Runtime;
use sigil::terminal::Screen;
use tokio::sync::mpsc::unbounded_channel;

use super::app::App;
use super::profile::info;
use super::view::root;
use crate::tools::Event;
use crate::tui::{Activity, Info, Msg};

mod alignment;
mod approval;
mod attach;
mod cards;
mod children;
mod composer_todos;
mod copy_notice;
mod effort;
mod footer;
mod form;
mod frames;
mod gutter;
mod kept;
mod mcp;
mod mention;
mod modals;
mod mode;
mod paths;
mod picker;
mod reflow;
mod secret;
mod settings;
mod strip;
mod subagents;
mod thinking;
mod typeahead;
mod typing;
mod usage;

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
        pick: Some(crate::endpoint::Pick::Flag),
        relay: Some("127.0.0.1:18080".to_owned()),
        session: "0123456789abcdef".to_owned(),
        ..info()
    }
}

fn paint(
    base: Info,
    cols: u16,
    rows: u16,
    state: State,
    setup: impl FnOnce(&App),
) -> sigil::terminal::Grid {
    paint_steps(base, (cols, rows), state, setup, &[])
        .pop()
        .expect("frame")
}

fn paint_steps(
    base: Info,
    (cols, rows): (u16, u16),
    state: State,
    setup: impl FnOnce(&App),
    steps: &[&dyn Fn(&App)],
) -> Vec<sigil::terminal::Grid> {
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
                patch: None,
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
        host.mount(move || root(tree));
        let mut screen = Screen::new(cols, rows);
        let mut router = EventRouter::new();
        let mut settle = || {
            for _ in 0..2 {
                screen
                    .frame(&mut host, &mut router, [], (cols, rows))
                    .expect("frame");
            }
            screen
                .frame(&mut host, &mut router, [], (cols, rows))
                .expect("frame")
                .clone()
        };
        let mut grids = vec![settle()];
        for step in steps {
            step(&app);
            app.refresh();
            grids.push(settle());
        }
        grids
    })
}

fn frame_with(
    base: Info,
    cols: u16,
    rows: u16,
    state: State,
    setup: impl FnOnce(&App),
) -> Vec<String> {
    paint(base, cols, rows, state, setup)
        .text()
        .lines()
        .map(str::to_owned)
        .collect()
}

fn screens(cols: u16, rows: u16, setup: impl FnOnce(&App), steps: &[&dyn Fn(&App)]) -> Vec<String> {
    paint_steps(busy_info(), (cols, rows), State::Idle, setup, steps)
        .iter()
        .map(sigil::terminal::Grid::text)
        .collect()
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
                !lines.iter().any(|line| line.contains("─  ─")),
                "{cols}x{rows} {state:?}: gap in the composer rule:\n{shown}"
            );
            assert!(
                find(&lines, "steer").is_none() && find(&lines, "esc cancel").is_none(),
                "{cols}x{rows} {state:?}: composer hint:\n{shown}"
            );
        }
    }
}

#[test]
fn tool_card_shows_at_every_size() {
    for (cols, rows) in SIZES {
        let lines = frame(cols, rows, State::Tool);
        assert!(
            find(&lines, "Read").is_some(),
            "{cols}x{rows}: no tool card:\n{}",
            lines.join("\n")
        );
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
    assert_eq!(shown.matches("thinking…").count(), 1, "{shown}");
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

#[test]
fn the_side_pane_does_not_add_a_full_height_rule() {
    use super::pane::{Layout, Pane};
    let grid = paint(busy_info(), 100, 30, State::Idle, |app| {
        app.layout.set(Layout {
            secondary: Some(Pane::Files),
            share: 30,
        });
    });
    let rails = (0..100usize)
        .filter(|&col| {
            (0..30)
                .filter(|&row| {
                    grid.get(col, row)
                        .is_some_and(|cell| cell.text == "│" || cell.text == "┃")
                })
                .count()
                > 20
        })
        .count();
    assert_eq!(rails, 0, "a full-height rule sits beside the transcript");
}

#[test]
fn your_line_is_the_text_you_sent() {
    let lines = frame(80, 24, State::Idle);
    let row = lines
        .iter()
        .find(|line| line.contains("Review the change"))
        .expect("user text");
    assert_eq!(row.trim(), "Review the change", "{row:?}");
}

#[test]
fn a_cleared_transcript_shows_the_note_in_place_of_the_prompt() {
    let shots = screens(80, 24, |_| {}, &[&|app: &App| app.update(Msg::Cleared)]);
    let last = shots.last().map(String::as_str).unwrap_or_default();
    assert!(
        last.contains("history cleared") && !last.contains("Review the change"),
        "{last}"
    );
}
