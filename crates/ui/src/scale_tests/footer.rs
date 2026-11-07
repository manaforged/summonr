use super::{State, busy_info, composer_top, find, frame_with, paint};
use summonr_agent::hub::{Fact, Intent, Msg};
use summonr_agent::tools::Event;

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
        app.update(Msg::Fact(Fact::Event(Event::Usage {
            prompt_tokens: 1200,
            completion_tokens: 300,
            reasoning_tokens: 0,
            cached_tokens: 800,
            cache_write_tokens: 0,
            millis: Some(2000),
            prefix_hash: String::new(),
            cost: Some(0.0412),
            side: false,
        })));
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
            app.update(Msg::Fact(Fact::Event(Event::Assistant {
                text: format!("line {at}"),
            })));
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
        app.update(Msg::Fact(Fact::Settings(summonr_agent::hub::Snapshot {
            model: "stub-model".to_owned(),
            context_window: Some(200_000),
            ..summonr_agent::hub::Snapshot::default()
        })));
    }));
    assert!(known.contains("ctx"), "{known}");
    let small = footer(&frame_with(busy_info(), 110, 30, State::Idle, |app| {
        app.update(Msg::Fact(Fact::Settings(summonr_agent::hub::Snapshot {
            model: "stub-model".to_owned(),
            context_window: Some(200_000),
            ..summonr_agent::hub::Snapshot::default()
        })));
        app.update(Msg::Fact(Fact::Event(Event::Usage {
            prompt_tokens: 1500,
            completion_tokens: 20,
            reasoning_tokens: 0,
            cached_tokens: 0,
            cache_write_tokens: 0,
            millis: None,
            prefix_hash: String::new(),
            cost: None,
            side: false,
        })));
    }));
    let ctx = small.split("cache").next().unwrap_or_default();
    assert!(ctx.contains("ctx") && ctx.contains("1%"), "{small}");
}

#[test]
fn an_unseated_session_shows_no_provider_instead_of_a_model() {
    let lines = frame_with(busy_info(), 110, 30, State::Idle, |app| {
        app.update(Msg::Fact(Fact::Settings(summonr_agent::hub::Snapshot {
            model: "glm-5.3-flash".to_owned(),
            endpoint: "zai".to_owned(),
            unconfigured: true,
            ..summonr_agent::hub::Snapshot::default()
        })));
    });
    let screen = lines.join("\n");
    assert!(screen.contains("no provider"), "{screen}");
    assert!(!screen.contains("glm-5.3-flash"), "{screen}");
}

#[test]
fn a_meter_card_opens_on_a_click_and_closes_on_a_click_outside() {
    use sigil::events::{Event, PointerPhase};
    use sigil::terminal::{Screen, cell_point};
    fn pointer(phase: PointerPhase, col: u16, row: u16) -> Event {
        let at = cell_point(col, row);
        match phase {
            PointerPhase::Down => Event::pointer_down(at),
            PointerPhase::Up => Event::pointer_up(at),
            _ => Event::pointer_drag(at),
        }
    }
    let shown = sigil::reactive::Owner::scope("meter-card", || {
        let workspace = tempfile::tempdir().unwrap();
        let (commands, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let app = super::super::app::App::new(busy_info(), workspace.path().to_owned(), commands);
        app.resize(110, 30);
        app.update(Msg::Intent(Intent::User(
            "started conversation".to_string(),
        )));
        app.refresh();
        let mut host = sigil::Host::new(sigil::prelude::HostMode::Cell);
        let tree = app.clone();
        host.mount(move || super::super::view::root(tree));
        let mut screen = Screen::new(110, 30);
        let mut text = String::new();
        for _ in 0..2 {
            text = screen.frame(&mut host, [], (110, 30)).unwrap().text();
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
                screen.frame(&mut host, [event], (110, 30)).unwrap();
            }
            let grid = screen.frame(&mut host, [], (110, 30)).unwrap();
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

#[test]
fn the_model_reads_in_the_same_ink_as_the_workspace_path() {
    let info = summonr_agent::hub::Info {
        workspace: "projects/demo".to_owned(),
        ..busy_info()
    };
    let grid = paint(info, 110, 34, State::Idle, |_| {});
    let shown = grid.text();
    let lines: Vec<&str> = shown.lines().collect();
    let ink = |needle: &str| {
        let row = lines
            .iter()
            .rposition(|line| line.contains(needle))
            .unwrap_or_else(|| panic!("{needle} on screen:\n{shown}"));
        let cells: Vec<String> = (0..grid.cols())
            .filter_map(|col| grid.get(col, row).map(|cell| cell.text().to_string()))
            .collect();
        let col = (0..cells.len())
            .find(|&col| cells[col..].concat().starts_with(needle) && !cells[col].is_empty())
            .unwrap_or_else(|| panic!("{needle} cells in row {row}"));
        grid.get(col, row).map(|cell| cell.fg)
    };
    assert_eq!(ink("stub-model"), ink("projects/demo"), "{shown}");
}

fn live(
    session: &str,
    status: summonr_agent::hub::Status,
    active: bool,
) -> summonr_agent::hub::Live {
    summonr_agent::hub::Live {
        session: session.to_owned(),
        title: session.to_owned(),
        workspace: std::path::PathBuf::from("/work"),
        status,
        active,
        parent: None,
    }
}

fn spent(app: &super::super::app::App) {
    app.update(Msg::Fact(Fact::Event(Event::Usage {
        prompt_tokens: 1200,
        completion_tokens: 300,
        reasoning_tokens: 0,
        cached_tokens: 800,
        cache_write_tokens: 0,
        millis: Some(2000),
        prefix_hash: String::new(),
        cost: Some(0.0412),
        side: false,
    })));
}

#[test]
fn the_footer_counts_other_sessions_and_the_ones_waiting_on_you() {
    use summonr_agent::hub::Status;
    let shown = |sessions: Vec<summonr_agent::hub::Live>| {
        footer(&frame_with(busy_info(), 140, 30, State::Idle, |app| {
            spent(app);
            app.update(Msg::Fact(Fact::Live(sessions)));
        }))
    };
    let waiting = shown(vec![
        live("this", Status::Working, true),
        live("b", Status::NeedsYou, false),
        live("c", Status::Idle, false),
    ]);
    assert!(
        waiting.contains("2 other sessions · 1 waiting on you · /resume"),
        "{waiting}"
    );
    let quiet = shown(vec![
        live("this", Status::NeedsYou, true),
        live("b", Status::Working, false),
    ]);
    assert!(quiet.contains("1 other session"), "{quiet}");
    assert!(!quiet.contains("waiting"), "{quiet}");
    let alone = shown(vec![live("this", Status::Working, true)]);
    assert!(!alone.contains("other session"), "{alone}");
}

#[test]
fn a_long_workspace_path_takes_the_room_the_footer_leaves() {
    let mut info = busy_info();
    info.workspace = format!("/home/someone/{}checkout-name", "nested/".repeat(20));
    let row = |window: Option<u32>| {
        footer(&frame_with(info.clone(), 110, 30, State::Idle, |app| {
            app.update(Msg::Fact(Fact::Settings(summonr_agent::hub::Snapshot {
                model: "stub-model".to_owned(),
                context_window: window,
                ..summonr_agent::hub::Snapshot::default()
            })));
            spent(app);
            app.update(Msg::Fact(Fact::Live(vec![
                live("this", summonr_agent::hub::Status::Working, true),
                live("b", summonr_agent::hub::Status::NeedsYou, false),
            ])));
        }))
    };
    for window in [None, Some(200_000)] {
        let line = row(window);
        for part in ["-name", "waiting on you", "$0.04", "cache"] {
            assert!(line.contains(part), "{part}: {line}");
        }
        assert!(line.trim_end().ends_with('%'), "{line}");
        assert!(line.trim_end().chars().count() >= 104, "{line}");
    }
}

fn meter_cells(grid: &sigil::terminal::Grid, caption: &str) -> Vec<sigil::terminal::Cell> {
    let text = grid.text();
    let row = text
        .lines()
        .collect::<Vec<_>>()
        .iter()
        .rposition(|line| line.contains("cache"))
        .unwrap_or_else(|| panic!("no footer:\n{text}"));
    let cells: Vec<sigil::terminal::Cell> = (0..grid.cols())
        .filter_map(|col| grid.get(col, row))
        .collect();
    let glyphs: Vec<&str> = cells.iter().map(sigil::terminal::Cell::text).collect();
    let start = (0..cells.len())
        .rfind(|&col| glyphs[col..].concat().starts_with(&format!("{caption} ")))
        .unwrap_or_else(|| panic!("no {caption} in the footer:\n{text}"))
        + caption.len()
        + 1;
    let end = (start..cells.len())
        .find(|&col| glyphs[col].starts_with(|ch: char| ch.is_ascii_digit()))
        .unwrap_or(cells.len());
    cells[start..end.saturating_sub(1)].to_vec()
}

fn meter_bar(cells: &[sigil::terminal::Cell], fill: sigil::terminal::Ink) -> String {
    let base = super::super::canvas::palette().base;
    let unseen = |ink: sigil::terminal::Ink| match ink {
        sigil::terminal::Ink::Exact(rgb) => (rgb.r, rgb.g, rgb.b) == (base.r, base.g, base.b),
        _ => true,
    };
    cells
        .iter()
        .map(|cell| match (cell.text(), cell.bg) {
            (_, ink) if ink == fill => '█',
            ("" | " ", ink) if unseen(ink) => ' ',
            _ => '─',
        })
        .collect()
}

fn metered(percent: u32) -> sigil::terminal::Grid {
    paint(busy_info(), 140, 30, State::Idle, |app| {
        app.update(Msg::Fact(Fact::Settings(summonr_agent::hub::Snapshot {
            model: "stub-model".to_owned(),
            context_window: Some(200_000),
            ..summonr_agent::hub::Snapshot::default()
        })));
        app.update(Msg::Fact(Fact::Event(Event::Usage {
            prompt_tokens: 2000 * percent,
            completion_tokens: 0,
            reasoning_tokens: 0,
            cached_tokens: 20 * percent * percent,
            cache_write_tokens: 0,
            millis: None,
            prefix_hash: String::new(),
            cost: None,
            side: false,
        })));
    })
}

#[test]
fn the_context_and_cache_meters_draw_a_whole_bar_for_each_percent() {
    let full = metered(100);
    for caption in ["ctx", "cache"] {
        let fill = meter_cells(&full, caption)[0].bg;
        for (percent, bar) in [
            (0, "        "),
            (1, "        "),
            (50, "████    "),
            (100, "████████"),
        ] {
            let grid = metered(percent);
            let cells = meter_cells(&grid, caption);
            let bar = bar.replace(' ', "─");
            assert_eq!(
                meter_bar(&cells, fill),
                bar,
                "{caption} at {percent}%: {:?}\n{}",
                cells
                    .iter()
                    .map(|cell| (cell.text(), cell.bg))
                    .collect::<Vec<_>>(),
                grid.text()
            );
        }
    }
}
