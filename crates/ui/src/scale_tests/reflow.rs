use sigil::Host;
use sigil::terminal::Screen;
use tokio::sync::mpsc::unbounded_channel;

use super::super::app::App;
use super::super::view::root;
use super::busy_info;
use summonr_agent::hub::{Fact, Msg};
use summonr_agent::tools::Event;

const ANSWER: &str = "The functions in calc.py:\n\n1. add(a, b) returns the sum\n2. sub(a, b) returns the difference\n3. mul(a, b) returns the product\n4. div(a, b) returns the quotient; exact int division returns an int, otherwise a float; raises ValueError on zero or NaN divisors and never returns infinity for any input";

fn frames(sizes: &[(u16, u16)]) -> String {
    sigil::reactive::Owner::scope("summonr-reflow", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, _receiver) = unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        let (cols, rows) = sizes[0];
        app.resize(cols, rows);
        app.update(Msg::Fact(Fact::Event(summonr_agent::Event::User {
            text: "list the functions in calc.py".to_owned(),
            images: Vec::new(),
        })));
        app.update(Msg::Fact(Fact::Event(Event::Assistant {
            text: ANSWER.to_owned(),
        })));
        app.refresh();
        let mut host = Host::new(sigil::prelude::HostMode::Cell);
        let tree = app.clone();
        host.mount(move || root(tree));
        let mut screen = Screen::new(cols, rows);
        let mut text = String::new();
        for &(cols, rows) in sizes {
            app.resize(cols, rows);
            for _ in 0..3 {
                text = screen
                    .frame(&mut host, [], (cols, rows))
                    .expect("frame")
                    .text();
            }
        }
        text
    })
}

fn spoken_words(screen: &str) -> Vec<&str> {
    screen.split_whitespace().collect()
}

#[test]
fn a_wrapped_answer_keeps_every_word_after_the_terminal_narrows() {
    let resized = frames(&[(160, 48), (100, 30)]);
    let shown = spoken_words(&resized);
    for word in spoken_words(ANSWER.lines().last().unwrap_or_default()) {
        assert!(
            shown.contains(&word),
            "{word:?} lost after resize:\n{resized}"
        );
    }
    let fresh = frames(&[(100, 30)]);
    assert_eq!(resized, fresh);
}

#[test]
fn a_wrapped_answer_matches_fresh_layout_when_narrowing_within_wide_fit() {
    let resized = frames(&[(160, 48), (110, 30)]);
    let fresh = frames(&[(110, 30)]);
    assert_eq!(resized, fresh);
}

#[test]
fn a_wrapped_answer_updates_edge_padding_across_roomy_rows() {
    let shrunk = frames(&[(100, 30), (100, 20)]);
    assert_eq!(shrunk, frames(&[(100, 20)]));
    let grown = frames(&[(100, 20), (100, 30)]);
    assert_eq!(grown, frames(&[(100, 30)]));
}
