use super::{State, busy_info, composer_top, find, frame_with};
use crate::tools::Event;
use crate::tui::Msg;

fn footer(lines: &[String]) -> String {
    lines
        .iter()
        .rfind(|line| line.contains("cache"))
        .cloned()
        .unwrap_or_default()
}

#[test]
fn the_footer_shows_spend_beside_the_meters_and_no_rate() {
    let lines = frame_with(busy_info(), 110, 30, State::Idle, |app| {
        app.update(Msg::Event(Event::Usage {
            prompt_tokens: 1200,
            completion_tokens: 300,
            reasoning_tokens: 0,
            cached_tokens: 800,
            millis: Some(2000),
            prefix_hash: String::new(),
            cost: Some(0.0412),
        }));
    });
    let line = footer(&lines);
    let spend = line.find("$0.04").unwrap_or_else(|| panic!("{line}"));
    let ctx = line.find("cache").unwrap_or_default();
    assert!(spend < ctx, "{line}");
    assert!(!line.contains("tok/s"), "{line}");
}

#[test]
fn a_short_transcript_keeps_the_composer_under_it_and_the_footer_at_the_bottom() {
    let lines = frame_with(busy_info(), 110, 40, State::Idle, |_| {});
    let reply = find(&lines, "Measured layout.").unwrap_or_else(|| panic!("{}", lines.join("\n")));
    let composer = composer_top(&lines);
    assert!(
        composer > reply && composer - reply <= 4,
        "composer at row {composer}, reply at row {reply}:\n{}",
        lines.join("\n")
    );
    let bottom = lines
        .iter()
        .rposition(|line| !line.trim().is_empty())
        .unwrap_or_default();
    assert_eq!(
        bottom,
        lines
            .iter()
            .rposition(|line| line.contains("cache"))
            .unwrap_or_default()
    );
    assert!(
        bottom >= 37,
        "footer at row {bottom}:\n{}",
        lines.join("\n")
    );
}

#[test]
fn a_long_transcript_keeps_the_composer_at_the_bottom() {
    let lines = frame_with(busy_info(), 110, 30, State::Idle, |app| {
        for at in 0..60 {
            app.update(Msg::Event(Event::Assistant {
                text: format!("line {at}"),
            }));
        }
    });
    let composer = composer_top(&lines);
    let footer = lines
        .iter()
        .rposition(|line| line.contains("cache"))
        .unwrap_or_default();
    assert!(
        footer - composer <= 4,
        "composer at {composer}, footer at {footer}:\n{}",
        lines.join("\n")
    );
    assert!(
        find(&lines, "line 59").is_some(),
        "the newest entry is not visible:\n{}",
        lines.join("\n")
    );
}

#[test]
fn the_context_meter_shows_only_when_the_window_is_known() {
    let unknown = footer(&frame_with(busy_info(), 110, 30, State::Idle, |_| {}));
    assert!(!unknown.contains("ctx"), "{unknown}");
    let known = footer(&frame_with(busy_info(), 110, 30, State::Idle, |app| {
        app.update(Msg::Settings(crate::tui::Snapshot {
            model: "stub-model".to_owned(),
            context_window: Some(200_000),
            ..crate::tui::Snapshot::default()
        }));
    }));
    assert!(known.contains("ctx"), "{known}");
    let small = footer(&frame_with(busy_info(), 110, 30, State::Idle, |app| {
        app.update(Msg::Settings(crate::tui::Snapshot {
            model: "stub-model".to_owned(),
            context_window: Some(200_000),
            ..crate::tui::Snapshot::default()
        }));
        app.update(Msg::Event(Event::Usage {
            prompt_tokens: 1500,
            completion_tokens: 20,
            reasoning_tokens: 0,
            cached_tokens: 0,
            millis: None,
            prefix_hash: String::new(),
            cost: None,
        }));
    }));
    let ctx = small.split("cache").next().unwrap_or_default();
    assert!(ctx.contains("ctx") && ctx.contains("1%"), "{small}");
}

#[test]
fn an_unseated_session_shows_no_provider_instead_of_a_model() {
    let lines = frame_with(busy_info(), 110, 30, State::Idle, |app| {
        app.update(Msg::Settings(crate::tui::Snapshot {
            model: "glm-5.3-flash".to_owned(),
            endpoint: "zai".to_owned(),
            unseated: true,
            ..crate::tui::Snapshot::default()
        }));
    });
    let screen = lines.join("\n");
    assert!(screen.contains("no provider"), "{screen}");
    assert!(!screen.contains("glm-5.3-flash"), "{screen}");
}

#[test]
fn a_meter_card_opens_on_a_click_and_closes_on_a_click_outside() {
    use sigil::events::{EventRouter, PointerPhase};
    use sigil::terminal::{Screen, pointer};
    let shown = sigil::reactive::Runtime::scope("meter-card", || {
        let workspace = tempfile::tempdir().unwrap();
        let (commands, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let app = super::super::app::App::new(busy_info(), workspace.path().to_owned(), commands);
        app.resize(110, 30);
        app.update(Msg::User("started conversation".to_string()));
        app.refresh();
        let mut host = sigil::Host::cells();
        let tree = app.clone();
        host.mount(move || super::super::view::root(tree));
        let mut screen = Screen::new(110, 30);
        let mut router = EventRouter::new();
        let mut text = String::new();
        for _ in 0..2 {
            text = screen
                .frame(&mut host, &mut router, [], (110, 30))
                .unwrap()
                .text();
        }
        let lines: Vec<String> = text.lines().map(str::to_owned).collect();
        let row = lines
            .iter()
            .rposition(|line| line.contains("cache"))
            .unwrap();
        let col = lines[row][..lines[row].rfind("cache").unwrap()]
            .chars()
            .count();
        let mut send = |phases: &[PointerPhase], col: usize, row: usize| {
            let (col, row) = (col as u16, row as u16);
            for phase in phases {
                let event = pointer(*phase, col, row);
                screen
                    .frame(&mut host, &mut router, [event], (110, 30))
                    .unwrap();
            }
            let grid = screen.frame(&mut host, &mut router, [], (110, 30)).unwrap();
            grid.text().contains("╭ cache")
        };
        let click = [PointerPhase::Move, PointerPhase::Down, PointerPhase::Up];
        let opened = send(&click, col + 2, row);
        send(&click, col + 2, row - 2);
        let inside = send(&[PointerPhase::Move], 2, 2);
        let outside = send(&click, 2, 2);
        (opened, inside, outside)
    });
    assert_eq!(
        shown,
        (true, true, false),
        "open, after a click inside and moving away, after a click outside"
    );
}
