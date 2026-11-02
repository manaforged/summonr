use sigil::Host;
use sigil::events::Event as SigilEvent;
use sigil::terminal::{Screen, cell_point};
use tokio::sync::mpsc::unbounded_channel;

use super::super::app::App;
use super::super::view::root;
use super::busy_info;
use crate::prefs::Visibility;
use crate::tools::Event;
use crate::tui::{Fact, Intent, Msg};

const COLS: u16 = 100;
const ROWS: u16 = 30;

fn row_of(screen: &[String], needle: &str) -> u16 {
    let row = screen
        .iter()
        .position(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("no `{needle}`:\n{}", screen.join("\n")));
    u16::try_from(row).unwrap()
}

#[test]
fn only_the_thinking_header_folds_the_reasoning() {
    sigil::reactive::Owner::scope("thinking-click", || {
        let workspace = tempfile::tempdir().unwrap();
        let mut info = busy_info();
        info.prefs.reasoning = Visibility::Expanded;
        let (commands, _receiver) = unbounded_channel();
        let app = App::new(info, workspace.path().to_owned(), commands);
        app.resize(COLS, ROWS);
        app.update(Msg::Intent(Intent::User("why".to_owned())));
        app.update(Msg::Fact(Fact::Event(Event::Reasoning {
            text: "copy this reasoning line".to_owned(),
        })));
        app.update(Msg::Fact(Fact::Event(Event::Assistant {
            text: "done".to_owned(),
        })));
        app.refresh();
        let mut host = Host::new(sigil::prelude::HostMode::Cell);
        let tree = app.clone();
        host.mount(move || root(tree));
        let mut screen = Screen::new(COLS, ROWS);
        let lines = |grid: &sigil::terminal::Grid| -> Vec<String> {
            grid.text().lines().map(str::to_owned).collect()
        };
        let size = (COLS, ROWS);
        screen.frame(&mut host, [], size).unwrap();
        let shown = lines(screen.frame(&mut host, [], size).unwrap());
        let body = row_of(&shown, "copy this reasoning line");
        screen
            .frame(&mut host, SigilEvent::click(cell_point(10, body)), size)
            .unwrap();
        let after = lines(screen.frame(&mut host, [], size).unwrap());
        assert_eq!(app.model.peek().prefs.reasoning, Visibility::Expanded);
        assert!(
            after
                .iter()
                .any(|line| line.contains("copy this reasoning line")),
            "{}",
            after.join("\n")
        );
        let header = row_of(&after, "✻ thinking");
        screen
            .frame(&mut host, SigilEvent::click(cell_point(4, header)), size)
            .unwrap();
        screen.frame(&mut host, [], size).unwrap();
        assert_eq!(app.model.peek().prefs.reasoning, Visibility::Collapsed);
    });
}
