use std::cell::RefCell;
use std::rc::Rc;

use sigil::Host;
use sigil::clipboard::{Clipboard, Delivery};
use sigil::events::Event as SigilEvent;
use sigil::terminal::{Screen, cell_point};
use tokio::sync::mpsc::unbounded_channel;

use super::super::app::App;
use super::super::view::root;
use super::busy_info;
use summonr_agent::hub::{Fact, Msg};
use summonr_agent::tools::Event;

const REPLY: &str = "The quick brown fox jumps over the lazy dog";

struct Recorder(Rc<RefCell<Vec<String>>>);

impl Clipboard for Recorder {
    fn set(&self, text: &str) -> Result<Delivery, sigil::Error> {
        self.0.borrow_mut().push(text.to_owned());
        Ok(Delivery::Confirmed)
    }

    fn get(&self) -> Option<String> {
        self.0.borrow().last().cloned()
    }
}

#[test]
fn dragging_across_a_reply_copies_it_and_says_so() {
    let (copied, footer) = sigil::reactive::Owner::scope("copy-notice", || {
        let copied = Rc::new(RefCell::new(Vec::new()));
        sigil::clipboard::install(Recorder(Rc::clone(&copied)));
        let workspace = tempfile::tempdir().unwrap();
        let (commands, _receiver) = unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        app.resize(100, 30);
        app.update(Msg::Fact(Fact::Event(summonr_agent::Event::User {
            text: "Say the pangram".to_owned(),
            images: Vec::new(),
        })));
        app.update(Msg::Fact(Fact::Event(Event::Assistant {
            text: REPLY.to_owned(),
        })));
        app.refresh();
        let mut host = Host::new(sigil::prelude::HostMode::Cell);
        let tree = app.clone();
        host.mount(move || root(tree));
        let mut screen = Screen::new(100, 30);
        screen.frame(&mut host, [], (100, 30)).unwrap();
        let grid = screen.frame(&mut host, [], (100, 30)).unwrap();
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
            SigilEvent::pointer_drag(cell_point(start, row)),
            SigilEvent::pointer_down(cell_point(start, row)),
            SigilEvent::pointer_drag(cell_point(start + 5, row)),
            SigilEvent::pointer_drag(cell_point(end, row)),
            SigilEvent::pointer_up(cell_point(end, row)),
        ] {
            screen.frame(&mut host, [event], (100, 30)).unwrap();
        }
        let grid = screen.frame(&mut host, [], (100, 30)).unwrap();
        let footer = grid.text().lines().last().unwrap_or_default().to_owned();
        let copied = copied.borrow().clone();
        (copied, footer)
    });
    assert_eq!(copied.last().map(String::as_str), Some(REPLY));
    assert!(footer.contains("copied 1 line"), "footer: {footer:?}");
}
