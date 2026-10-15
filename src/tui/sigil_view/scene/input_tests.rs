use crate::tui::sigil_view::{app::App, profile::info, view::root};
use sigil::reactive::Runtime;
use sigil::terminal::{Cell, Ink};
use tokio::sync::mpsc::unbounded_channel;

#[test]
fn the_splash_card_is_a_solid_plate() {
    let mut cells = vec![Cell::blank(); 32];
    super::dim_rect(&mut cells, 8, [0, 0, 8, 4], true);
    assert!(cells.iter().all(|cell| matches!(
        cell.bg,
        Ink::Exact(colour) if colour == super::PLATE
    )));
}

#[test]
fn the_splash_card_covers_the_stars() {
    Runtime::scope("splash-plate", || {
        let workspace = tempfile::tempdir().unwrap();
        std::fs::create_dir(workspace.path().join(".git")).unwrap();
        std::fs::write(workspace.path().join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        let (commands, _receiver) = unbounded_channel();
        let app = App::new(info(), workspace.path().to_owned(), commands);
        app.splash_revealed.set(true);
        let cols = 80_u16;
        let rows = 24_u16;
        let mut cells = vec![
            Cell {
                text: "✦".into(),
                fg: Ink::Exact(super::SCRIM),
                bg: Ink::Exact(super::SCRIM),
                ..Cell::blank()
            };
            usize::from(cols) * usize::from(rows)
        ];
        super::super::compose(&mut cells, cols, rows, &app);
        let plate = |cell: &Cell| matches!(cell.bg, Ink::Exact(colour) if colour == super::PLATE);
        let mut bounds: Option<(usize, usize, usize, usize)> = None;
        for row in 0..usize::from(rows) {
            for col in 0..usize::from(cols) {
                let cell = &cells[row * usize::from(cols) + col];
                if !plate(cell) {
                    continue;
                }
                bounds = Some(match bounds {
                    None => (col, row, col, row),
                    Some((x0, y0, x1, y1)) => (x0.min(col), y0.min(row), x1.max(col), y1.max(row)),
                });
            }
        }
        let Some((x0, y0, x1, y1)) = bounds else {
            panic!("splash card missing");
        };
        assert!(x1 > x0 + 1 && y1 > y0 + 1, "splash card is a sliver");
        let edge =
            |cell: &Cell| matches!(cell.fg, Ink::Exact(colour) if colour == super::palette().gray);
        let at = |col: usize, row: usize| &cells[row * usize::from(cols) + col];
        assert_eq!(at(x0, y0).text, "╭", "top left");
        assert_eq!(at(x1, y0).text, "╮", "top right");
        assert_eq!(at(x0, y1).text, "╰", "bottom left");
        assert_eq!(at(x1, y1).text, "╯", "bottom right");
        for col in x0 + 1..x1 {
            assert_eq!(at(col, y0).text, "─", "top edge {col}");
            assert_eq!(at(col, y1).text, "─", "bottom edge {col}");
            assert!(edge(at(col, y0)), "top edge colour {col}");
        }
        for row in y0 + 1..y1 {
            assert_eq!(at(x0, row).text, "│", "left edge {row}");
            assert_eq!(at(x1, row).text, "│", "right edge {row}");
        }
        for row in y0..=y1 {
            for col in x0..=x1 {
                let cell = at(col, row);
                assert!(plate(cell), "card hole at {col},{row} bg={:?}", cell.bg);
                assert_ne!(cell.text, "✦", "star shows through the card at {col},{row}");
            }
        }
    });
}

fn render(app: &App, cols: u16, rows: u16) -> String {
    let mut host = sigil::Host::cells();
    let tree = app.clone();
    host.mount(move || root(tree));
    let mut screen = sigil::terminal::Screen::new(cols, rows);
    let mut router = sigil::events::EventRouter::new();
    for _ in 0..3 {
        screen
            .frame(&mut host, &mut router, [], (cols, rows))
            .expect("splash frame");
    }
    screen
        .frame(&mut host, &mut router, [], (cols, rows))
        .expect("settled splash frame")
        .text()
}

#[test]
fn splash_composer_grows_for_multiline_and_wrapped_prompts_without_leaving() {
    Runtime::scope("splash-growing-composer", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, _receiver) = unbounded_channel();
        let app = App::new(info(), workspace.path().to_owned(), commands);
        app.splash.set(true);
        app.resize(80, 24);
        app.edit("first");
        let single = render(&app, 80, 24);
        let header = |text: &str| text.lines().position(|line| line.contains("● auto"));
        let top = header(&single).expect("single-line composer");
        for draft in [
            "first\nsecond",
            "first wrapped prompt wrapped prompt wrapped prompt wrapped prompt wrapped prompt tail",
        ] {
            app.edit(draft);
            assert!(app.splash.peek(), "typing left the splash");
            let shown = render(&app, 80, 24);
            assert_eq!(header(&shown), Some(top), "{shown}");
            assert!(shown.contains("first"), "{shown}");
            let tail = if draft.contains('\n') {
                "second"
            } else {
                "tail"
            };
            assert!(shown.contains(tail), "{shown}");
            app.resize(60, 20);
            assert!(app.splash.peek(), "resize left the splash");
            let shown = render(&app, 60, 20);
            assert!(shown.contains(tail), "{shown}");
            app.resize(80, 24);
        }
    });
}

#[test]
fn a_long_splash_draft_grows_down_under_the_card() {
    Runtime::scope("splash-draft-grows-down", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, _receiver) = unbounded_channel();
        let app = App::new(info(), workspace.path().to_owned(), commands);
        app.splash.set(true);
        app.splash_revealed.set(true);
        let rows = |text: &str, mark: &str| text.lines().position(|line| line.contains(mark));
        for (cols, height) in [(120_u16, 40_u16), (223, 67)] {
            app.resize(cols, height);
            app.edit("first");
            let single = render(&app, cols, height);
            let top = rows(&single, "● auto").expect("single-line composer");
            app.edit(&format!("{}tail", "breath of fire pixel style ".repeat(8)));
            let shown = render(&app, cols, height);
            let card = rows(&shown, "╰").expect("card bottom");
            let header = rows(&shown, "● auto").expect("composer header");
            assert_eq!(header, top, "{shown}");
            assert!(header > card, "{shown}");
            assert!(shown.contains("tail"), "{shown}");
            app.edit("/");
            let listed = render(&app, cols, height);
            assert!(listed.contains("/compact"), "{listed}");
            assert_eq!(rows(&listed, "● auto"), Some(top), "{listed}");
        }
        app.resize(80, 24);
        app.edit(&format!("{}tail", "line\n".repeat(30)));
        let shown = render(&app, 80, 24);
        assert!(shown.contains("tail"), "{shown}");
        assert!(rows(&shown, "● auto").is_some(), "{shown}");
    });
}

#[test]
fn slash_and_mention_drafts_keep_the_splash() {
    Runtime::scope("splash-typeahead", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, mut receiver) = unbounded_channel();
        let app = App::new(info(), workspace.path().to_owned(), commands);
        app.viewport.set((100, 30));
        app.splash.set(true);
        for draft in ["/", "/he", "see @src"] {
            app.edit(draft);
            assert!(app.splash.peek(), "{draft} left the splash");
        }
        app.edit("/sett");
        let shown = render(&app, 100, 30);
        assert!(
            shown.contains("commands") && shown.contains("/settings"),
            "{shown}"
        );
        assert!(
            shown.contains("/sett") && shown.contains("● auto"),
            "{shown}"
        );
        assert!(app.complete());
        assert_eq!(app.draft.peek(), "/settings");
        assert!(app.splash.peek(), "completion left the splash");
        app.edit("first line\nsecond line");
        assert!(app.splash.peek());
        app.submit();
        let submitted = std::iter::from_fn(|| receiver.try_recv().ok()).find_map(|command| {
            if let crate::tui::Cmd::Submit(input) = command {
                Some(input.text)
            } else {
                None
            }
        });
        assert_eq!(submitted.as_deref(), Some("first line\nsecond line"));
        assert!(
            !app.splash.peek(),
            "submitted transcript did not leave the splash"
        );
    });
}

#[test]
fn splash_hides_usage_meters_until_the_first_prompt_even_with_a_known_window() {
    Runtime::scope("splash-meter-lifecycle", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, _receiver) = unbounded_channel();
        let app = App::new(info(), workspace.path().to_owned(), commands);
        app.resize(110, 30);
        app.splash.set(true);
        app.update(crate::tui::Msg::Settings(crate::tui::Snapshot {
            model: "profile".to_string(),
            context_window: Some(200_000),
            ..crate::tui::Snapshot::default()
        }));
        let fresh = render(&app, 110, 30);
        assert!(
            !fresh.contains("ctx") && !fresh.contains("cache"),
            "{fresh}"
        );
        app.edit("start this conversation");
        app.submit();
        let started = render(&app, 110, 30);
        assert!(
            started.contains("ctx") && started.contains("cache"),
            "{started}"
        );
        app.update(crate::tui::Msg::Reset);
        let reset = render(&app, 110, 30);
        assert!(
            !reset.contains("ctx") && !reset.contains("cache"),
            "{reset}"
        );
    });
}

#[test]
fn a_server_prompt_line_is_sent_as_a_turn_and_an_unknown_command_is_not() {
    Runtime::scope("slash-turn", || {
        let workspace = tempfile::tempdir().unwrap();
        let (commands, mut receiver) = unbounded_channel();
        let app = App::new(info(), workspace.path().to_owned(), commands);
        let mut send = |line: &str| {
            app.draft.set(line.to_string());
            app.submit();
            let mut turns = 0;
            while let Ok(command) = receiver.try_recv() {
                turns += usize::from(
                    matches!(command, crate::tui::Cmd::Submit(input) if input.text == line),
                );
            }
            turns
        };
        assert_eq!(send("/docs:search tokio"), 1);
        assert_eq!(send("/nosuchcommand now"), 0);
    });
}

fn splash_screen(cols: u16, rows: u16) -> (App, sigil::Host, sigil::terminal::Screen) {
    let workspace = tempfile::tempdir().unwrap();
    let (commands, _receiver) = unbounded_channel();
    let app = App::new(info(), workspace.path().to_owned(), commands);
    app.model_mut(|model| model.prefs.animations = true);
    app.splash.set(true);
    app.splash_revealed.set(true);
    app.resize(cols, rows);
    app.animate();
    let mut host = sigil::Host::cells();
    let tree = app.clone();
    host.mount(move || root(tree));
    (app, host, sigil::terminal::Screen::new(cols, rows))
}

#[test]
fn a_model_change_on_the_splash_keeps_the_star_clock() {
    Runtime::scope("splash-clock", || {
        let (app, mut host, mut screen) = splash_screen(80, 24);
        let mut router = sigil::events::EventRouter::new();
        screen.set_dt(0.1);
        let mut frames = |count: usize| {
            for _ in 0..count {
                screen.frame(&mut host, &mut router, [], (80, 24)).unwrap();
            }
        };
        frames(5);
        let clock = app.sky_clock.peek().expect("a splash clock");
        let before = clock.peek();
        app.model_mut(|model| model.prefs.reasoning = crate::prefs::Visibility::Expanded);
        frames(5);
        let after = app.sky_clock.peek().expect("a splash clock");
        assert!(after == clock, "a model change replaced the star clock");
        assert!(
            after.peek() > before,
            "the star clock went back from {before} to {}",
            after.peek()
        );
    });
}

fn named(key: sigil::prelude::Key) -> sigil::events::ShellEvent {
    use sigil::events::{
        KeyEvent, KeyEventInit, KeyPhase, LogicalKey, ModifiersState, PhysicalKey,
    };
    sigil::events::ShellEvent::Key(KeyEvent::new(KeyEventInit {
        key: LogicalKey::named(key),
        physical: PhysicalKey::Unidentified,
        modifiers: ModifiersState::empty(),
        repeat: false,
        phase: KeyPhase::Down,
        text: None,
    }))
}

#[test]
fn the_splash_keeps_the_line_stack_s_when_f2_is_pressed() {
    Runtime::scope("splash-mark", || {
        let (_app, mut host, mut screen) = splash_screen(120, 40);
        let mut router = sigil::events::EventRouter::new();
        let mut shown = |events: Vec<sigil::events::ShellEvent>| {
            screen
                .frame(&mut host, &mut router, events, (120, 40))
                .unwrap();
            screen.frame(&mut host, &mut router, [], (120, 40)).unwrap();
            screen.text()
        };
        for events in [vec![], vec![named(sigil::prelude::Key::F2)]] {
            let shown = shown(events);
            assert!(
                shown.contains('┏') && shown.contains('┛') && shown.contains('┌'),
                "{shown}"
            );
        }
    });
}
