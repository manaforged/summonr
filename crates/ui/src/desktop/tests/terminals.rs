use std::cell::RefCell;
use std::time::{Duration, Instant};

use sigil::events::{Key, ModifiersState};

use super::harness::{Step, command, named, press, steps, typed};
use super::in_window;
use crate::desktop::App;
use crate::pane::Pane;

pub(super) type Pump = &'static RefCell<sigil::tasks::AsyncRuntime>;

pub(super) fn pumped() -> Pump {
    let runtime = sigil::tasks::AsyncRuntime::new().expect("runtime");
    runtime.install_current();
    Box::leak(Box::new(RefCell::new(runtime)))
}

fn screen_until(runtime: Pump, app: &App, wanted: impl Fn(&[String]) -> bool) -> Vec<String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        runtime.borrow_mut().pump();
        let screen = app
            .window
            .terminals
            .list
            .peek()
            .first()
            .map(|term| term.session.lines.get())
            .unwrap_or_default();
        if wanted(&screen) || Instant::now() > deadline {
            return screen;
        }
        std::thread::park_timeout(Duration::from_millis(20));
    }
}

fn line(keys: &str) -> Vec<Step> {
    let mut steps: Vec<Step> = typed(keys).into_iter().map(Step::Key).collect();
    steps.push(Step::Key(named(Key::Enter, ModifiersState::empty())));
    steps
}

#[test]
fn a_new_terminal_fills_the_pane_and_runs_what_you_type() {
    if !in_window("terminals::a_new_terminal_fills_the_pane_and_runs_what_you_type") {
        return;
    }
    let runtime = pumped();
    let mut keys = vec![Step::Key(command("t"))];
    keys.extend(line("echo typed-$((40+2))"));
    let seen = steps(
        |_| {},
        keys,
        |app| {
            let screen = screen_until(runtime, app, |lines| {
                lines.iter().any(|line| line.trim() == "typed-42")
            });
            (
                app.window.showing.peek().is_some(),
                app.window.terminals.list.peek().len(),
                screen,
            )
        },
    );
    assert!(
        seen.shows(|text| text.contains("typed-42")),
        "not painted: {}",
        seen.text()
    );
    let (showing, count, screen) = seen.read;
    assert!(showing, "the pane does not show the terminal");
    assert_eq!(count, 1);
    assert!(
        screen.iter().any(|line| line.trim() == "typed-42"),
        "{screen:?}"
    );
}

#[test]
fn control_c_reaches_the_program_and_not_the_agent() {
    if !in_window("terminals::control_c_reaches_the_program_and_not_the_agent") {
        return;
    }
    let runtime = pumped();
    let mut keys = vec![Step::Key(command("t"))];
    keys.extend(line("echo ready-$((1+1)); sleep 30; echo slept"));
    keys.push(Step::Wait(Box::new(move |app| {
        screen_until(runtime, app, |lines| {
            lines.iter().any(|line| line.trim() == "ready-2")
        });
    })));
    for _ in 0..2 {
        keys.push(Step::Key(press(
            sigil::events::LogicalKey::character("c"),
            ModifiersState::CONTROL,
            None,
        )));
    }
    keys.extend(line("echo after-$((1+1))"));
    let seen = steps(
        |_| {},
        keys,
        |app| {
            let screen = screen_until(runtime, app, |lines| {
                lines.iter().any(|line| line.trim() == "after-2")
            });
            (screen, !app.quit.peek())
        },
    );
    let (screen, open) = seen.read;
    assert!(
        screen.iter().any(|line| line.trim() == "after-2"),
        "{screen:?}"
    );
    assert!(open, "the window took the interrupt and quit");
}

#[test]
fn opening_an_agent_goes_back_to_it_and_keeps_the_terminal() {
    if !in_window("terminals::opening_an_agent_goes_back_to_it_and_keeps_the_terminal") {
        return;
    }
    pumped();
    let seen = steps(
        |_| {},
        vec![Step::Key(command("t")), Step::Key(command("n"))],
        |app| {
            (
                app.window.showing.peek(),
                app.window.terminals.list.peek().len(),
            )
        },
    );
    assert_eq!(seen.read, (None, 1));
}

#[test]
fn closing_a_terminal_removes_it_and_returns_to_the_agent() {
    if !in_window("terminals::closing_a_terminal_removes_it_and_returns_to_the_agent") {
        return;
    }
    pumped();
    let seen = steps(
        |_| {},
        vec![Step::Key(command("t")), Step::Click("Close terminal")],
        |app| {
            (
                app.window.showing.peek(),
                app.window.terminals.list.peek().len(),
            )
        },
    );
    assert_eq!(seen.read, (None, 0));
}

#[test]
fn the_window_keys_still_work_from_inside_a_terminal() {
    if !in_window("terminals::the_window_keys_still_work_from_inside_a_terminal") {
        return;
    }
    pumped();
    let backtick = press(
        sigil::events::LogicalKey::character("`"),
        ModifiersState::CONTROL,
        None,
    );
    let seen = steps(
        |_| {},
        vec![Step::Key(command("t")), Step::Key(backtick)],
        |app| app.layout.peek().secondary,
    );
    assert_eq!(seen.read, Some(Pane::Terminal));
}

#[test]
fn every_new_terminal_is_told_the_size_of_its_pane() {
    if !in_window("terminals::every_new_terminal_is_told_the_size_of_its_pane") {
        return;
    }
    let runtime = pumped();
    let mut keys = vec![Step::Key(command("t")), Step::Key(command("t"))];
    keys.extend(line("echo ready-$((1+1))"));
    keys.push(Step::Wait(Box::new(move |app| {
        let list = app.window.terminals.list.peek();
        let second = list.last().expect("second terminal");
        drop(second.session.write(b"stty size\r"));
        drop(list);
    })));
    let seen = steps(
        |_| {},
        keys,
        |app| {
            let last = app
                .window
                .terminals
                .list
                .peek()
                .last()
                .map(std::rc::Rc::clone)
                .expect("terminal");
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                runtime.borrow_mut().pump();
                let screen = last.session.lines.get();
                let answer = screen
                    .iter()
                    .map(|line| line.trim().to_owned())
                    .find(|line| {
                        let mut words = line.split(' ');
                        let number = |word: Option<&str>| {
                            word.is_some_and(|word| word.parse::<u16>().is_ok())
                        };
                        number(words.next()) && number(words.next()) && words.next().is_none()
                    });
                if answer.is_some() || Instant::now() > deadline {
                    return answer;
                }
                std::thread::park_timeout(Duration::from_millis(20));
            }
        },
    );
    assert_ne!(
        seen.read.as_deref(),
        Some("24 80"),
        "the second terminal kept the default size"
    );
    assert!(seen.read.is_some(), "stty never answered");
}

#[derive(Default)]
struct Held(std::rc::Rc<RefCell<String>>);

impl sigil::clipboard::Clipboard for Held {
    fn get(&self) -> Option<String> {
        Some(self.0.borrow().clone())
    }

    fn set(&self, text: &str) -> sigil::AppResult<sigil::clipboard::Delivery> {
        *self.0.borrow_mut() = text.to_owned();
        Ok(sigil::clipboard::Delivery::Confirmed)
    }
}

#[test]
fn dragging_over_output_selects_it_for_copy() {
    if !in_window("terminals::dragging_over_output_selects_it_for_copy") {
        return;
    }
    let runtime = pumped();
    let held = Held::default();
    let copied = std::rc::Rc::clone(&held.0);
    sigil::clipboard::install(held);
    let mut keys = vec![Step::Key(command("t"))];
    keys.extend(line("clear; echo select-$((1+1))"));
    keys.push(Step::Wait(Box::new(move |app| {
        screen_until(runtime, app, |lines| {
            lines.iter().any(|line| line.trim() == "select-2")
        });
    })));
    keys.push(Step::Drag((256.0, 58.0), (900.0, 58.0)));
    keys.push(Step::Key(command("c")));
    let seen = steps(|_| {}, keys, |_| ());
    assert_eq!(copied.borrow().as_str(), "select-2", "{}", seen.text());
}

fn exit_row(code: &str) -> Vec<String> {
    let runtime = pumped();
    let mut keys = vec![Step::Key(command("t"))];
    keys.extend(line(&format!("exit {code}")));
    keys.push(Step::Wait(Box::new(move |app| {
        let deadline = Instant::now() + Duration::from_secs(10);
        while app
            .window
            .terminals
            .list
            .peek()
            .first()
            .is_some_and(|term| term.session.exited.peek().is_none())
            && Instant::now() < deadline
        {
            runtime.borrow_mut().pump();
            std::thread::park_timeout(Duration::from_millis(20));
        }
    })));
    steps(|app| app.window.sidebar.set(true), keys, |_| ()).texts
}

#[test]
fn a_terminal_whose_program_fails_says_failed_in_the_sidebar() {
    if !in_window("terminals::a_terminal_whose_program_fails_says_failed_in_the_sidebar") {
        return;
    }
    let texts = exit_row("3");
    assert!(texts.iter().any(|text| text == "failed"), "{texts:?}");
    assert!(!texts.iter().any(|text| text == "exited"), "{texts:?}");
}

#[test]
fn a_terminal_whose_program_ends_cleanly_says_exited() {
    if !in_window("terminals::a_terminal_whose_program_ends_cleanly_says_exited") {
        return;
    }
    let texts = exit_row("0");
    assert!(texts.iter().any(|text| text == "exited"), "{texts:?}");
    assert!(!texts.iter().any(|text| text == "failed"), "{texts:?}");
}
