use std::cell::RefCell;
use std::rc::Rc;

use sigil::Host;
use sigil::clipboard::{ClipError, Clipboard, Delivery};
use sigil::events::{EventRouter, PointerPhase};
use sigil::reactive::Runtime;
use sigil::terminal::{Screen, pointer};
use tokio::sync::mpsc::unbounded_channel;

use super::super::app::App;
use super::super::view::root;
use super::busy_info;
use crate::tools::Event;
use crate::tui::Msg;

const REPLY: &str = "The quick brown fox jumps over the lazy dog";

struct Recorder(Rc<RefCell<Vec<String>>>);

impl Clipboard for Recorder {
    fn set(&self, text: &str) -> Result<Delivery, ClipError> {
        self.0.borrow_mut().push(text.to_owned());
        Ok(Delivery::Confirmed)
    }

    fn get(&self) -> Option<String> {
        self.0.borrow().last().cloned()
    }
}

#[test]
fn dragging_across_a_reply_copies_it_and_says_so() {
    let (copied, footer) = Runtime::scope("copy-notice", || {
        let copied = Rc::new(RefCell::new(Vec::new()));
        sigil::clipboard::install(Recorder(Rc::clone(&copied)));
        let workspace = tempfile::tempdir().unwrap();
        let (commands, _receiver) = unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        app.resize(100, 30);
        app.update(Msg::User("Say the pangram".to_owned()));
        app.update(Msg::Event(Event::Assistant {
            text: REPLY.to_owned(),
        }));
        app.refresh();
        let mut host = Host::cells();
        let tree = app.clone();
        host.mount(move || root(tree));
        let mut screen = Screen::new(100, 30);
        let mut router = EventRouter::new();
        screen.frame(&mut host, &mut router, [], (100, 30)).unwrap();
        let grid = screen.frame(&mut host, &mut router, [], (100, 30)).unwrap();
        let text = grid.text();
        let (row, line) = text
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains("quick brown"))
            .unwrap();
        let start = line[..line.find("The quick").unwrap()].chars().count() as u16;
        let end = start + REPLY.len() as u16 - 1;
        let row = row as u16;
        for event in [
            pointer(PointerPhase::Move, start, row),
            pointer(PointerPhase::Down, start, row),
            pointer(PointerPhase::Move, start + 5, row),
            pointer(PointerPhase::Move, end, row),
            pointer(PointerPhase::Up, end, row),
        ] {
            screen
                .frame(&mut host, &mut router, [event], (100, 30))
                .unwrap();
        }
        let grid = screen.frame(&mut host, &mut router, [], (100, 30)).unwrap();
        let footer = grid.text().lines().last().unwrap_or_default().to_owned();
        let copied = copied.borrow().clone();
        (copied, footer)
    });
    assert_eq!(copied.last().map(String::as_str), Some(REPLY));
    assert!(footer.contains("copied 1 line"), "footer: {footer:?}");
}
