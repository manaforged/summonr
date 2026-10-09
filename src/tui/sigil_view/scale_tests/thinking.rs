use sigil::Host;
use sigil::events::EventRouter;
use sigil::reactive::Runtime;
use sigil::terminal::{Screen, click};
use tokio::sync::mpsc::unbounded_channel;

use super::super::app::App;
use super::super::view::root;
use super::busy_info;
use crate::prefs::Visibility;
use crate::tools::Event;
use crate::tui::Msg;

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
    Runtime::scope("thinking-click", || {
        let workspace = tempfile::tempdir().unwrap();
        let mut info = busy_info();
        info.prefs.reasoning = Visibility::Expanded;
        let (commands, _receiver) = unbounded_channel();
        let app = App::new(info, workspace.path().to_owned(), commands);
        app.resize(COLS, ROWS);
        app.update(Msg::User("why".to_owned()));
        app.update(Msg::Event(Event::Reasoning {
            text: "copy this reasoning line".to_owned(),
        }));
        app.update(Msg::Event(Event::Assistant {
            text: "done".to_owned(),
        }));
        app.refresh();
        let mut host = Host::cells();
        let tree = app.clone();
        host.mount(move || root(tree));
        let mut screen = Screen::new(COLS, ROWS);
        let mut router = EventRouter::new();
        let lines = |grid: &sigil::terminal::Grid| -> Vec<String> {
            grid.text().lines().map(str::to_owned).collect()
        };
        let size = (COLS, ROWS);
        screen.frame(&mut host, &mut router, [], size).unwrap();
        let shown = lines(screen.frame(&mut host, &mut router, [], size).unwrap());
        let body = row_of(&shown, "copy this reasoning line");
        screen
            .frame(&mut host, &mut router, click(10, body), size)
            .unwrap();
        let after = lines(screen.frame(&mut host, &mut router, [], size).unwrap());
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
            .frame(&mut host, &mut router, click(4, header), size)
            .unwrap();
        screen.frame(&mut host, &mut router, [], size).unwrap();
        assert_eq!(app.model.peek().prefs.reasoning, Visibility::Collapsed);
    });
}
