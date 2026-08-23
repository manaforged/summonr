use std::path::PathBuf;
use std::time::Instant;

use crossterm::event::{
    KeyCode, KeyEvent as TerminalKeyEvent, KeyModifiers, MouseEvent, MouseEventKind,
};
use sigil::prelude::{Cx, Key, Rgba8};
use sigil_hal::Point;
use sigil_tui::{EventRouter, Grid, Host, Ink, Screen};
use tokio::sync::mpsc::unbounded_channel;
use unicode_width::UnicodeWidthStr;

use super::super::{Cmd, Info, Msg};
use super::app::App;
use super::chat;
use super::model::Entry;
use super::view::root;
use crate::prefs::Prefs;
use crate::tools::Event;

fn info() -> Info {
    Info {
        version: "0.1.0".to_string(),
        model: "glm-5.3".to_string(),
        relay: Some("example.invalid".to_string()),
        session: "session-12345".to_string(),
        endpoint: "glm".to_string(),
        prefs: Prefs {
            splash: false,
            ..Prefs::default()
        },
        plan: false,
        strict: true,
        workspace: "~/src/mage".to_string(),
        task: None,
    }
}

fn apply(app: &App, msg: Msg) {
    app.update(msg);
    app.refresh();
}

fn splash_frame(theme: &str, width: u16, height: u16, draft: &str) -> Grid {
    sigil::reactive::Runtime::scope("mage-splash-visual-test", || {
        let (cmd, _rx) = unbounded_channel();
        let mut start = info();
        start.prefs.splash = true;
        start.prefs.theme = theme.to_string();
        let app = App::new(start, PathBuf::from("/tmp/mage-view-test"), cmd);
        app.splash_revealed.set(true);
        app.resize(width, height);
        app.refresh();
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(width, height);
        screen
            .frame(&mut host, &mut router, [], (width, height))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        app.replace(draft.to_string());
        router.focus_next(host.cx());
        screen
            .frame(&mut host, &mut router, [], (width, height))
            .unwrap_or_else(|error| panic!("layout failed: {error}"))
            .clone()
    })
}

fn has_color(frame: &Grid, color: Rgba8) -> bool {
    let ink = Ink::Exact(color);
    (0..frame.rows()).any(|row| {
        frame
            .row(row)
            .iter()
            .any(|cell| cell.fg == ink || cell.bg == ink)
    })
}

#[test]
fn slash_menu_is_visible_on_the_splash() {
    sigil::reactive::Runtime::scope("mage-splash-command-menu-test", || {
        let (cmd, _rx) = unbounded_channel();
        let mut start = info();
        start.prefs.splash = true;
        let app = App::new(start, PathBuf::from("/tmp/mage-view-test"), cmd);
        app.splash_revealed.set(true);
        app.resize(100, 32);
        app.replace("/".to_string());
        app.refresh();
        let tree = app.clone();
        let frame = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 100, 32, &[])
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        if std::env::var_os("MAGE_DUMP_FRAME").is_some() {
            println!("splash commands 100x32:\n{frame}");
        }

        assert!(frame.contains("commands"), "menu missing:\n{frame}");
        assert!(frame.contains("› /help"), "selection missing:\n{frame}");
        assert!(frame.contains("/model"), "model command missing:\n{frame}");
        assert!(frame.contains("❯ /"), "composer was covered:\n{frame}");
        let prompt_at = frame.lines().position(|line| line.contains("❯ /"));
        let menu_at = frame.lines().position(|line| line.contains("╭─commands"));
        match (prompt_at, menu_at) {
            (Some(prompt_at), Some(menu_at)) => assert!(
                menu_at > prompt_at,
                "slash menu must sit below the splash composer:\n{frame}"
            ),
            _ => panic!("composer or menu missing:\n{frame}"),
        }
    });
}

#[test]
fn slash_menu_is_visible_after_leaving_splash() {
    sigil::reactive::Runtime::scope("mage-chat-command-menu-test", || {
        let (cmd, _rx) = unbounded_channel();
        let mut start = info();
        start.prefs.splash = true;
        let app = App::new(start, PathBuf::from("/tmp/mage-view-test"), cmd);
        app.splash_revealed.set(true);
        app.resize(100, 32);
        app.splash.set(false);
        app.replace("/".to_string());
        app.refresh();
        let tree = app.clone();
        let frame = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 100, 32, &[])
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        if std::env::var_os("MAGE_DUMP_FRAME").is_some() {
            println!("chat after splash 100x32:\n{frame}");
        }

        assert!(frame.contains("commands"), "chat menu missing:\n{frame}");
        assert!(frame.contains("› /help"), "selection missing:\n{frame}");
        assert!(frame.contains("❯ /"), "composer was covered:\n{frame}");
    });
}

#[test]
fn slash_menu_keeps_compact_chat_composer_visible() {
    sigil::reactive::Runtime::scope("mage-compact-command-menu-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(40, 12);
        app.replace("/".to_string());
        app.refresh();
        let tree = app.clone();
        let frame = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 40, 12, &[])
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        if std::env::var_os("MAGE_DUMP_FRAME").is_some() {
            println!("compact chat commands 40x12:\n{frame}");
        }

        assert!(frame.contains("commands"), "menu missing:\n{frame}");
        assert!(frame.contains("❯ /"), "composer was covered:\n{frame}");
    });
}

#[path = "sigil_view/app_tests.rs"]
mod app_tests;
#[path = "sigil_view/chat_tests.rs"]
mod chat_tests;
#[path = "sigil_view/scene_tests.rs"]
mod scene_tests;
#[path = "sigil_view/scroll_tests.rs"]
mod scroll_tests;
#[path = "sigil_view/settings_tests.rs"]
mod settings_tests;
#[path = "sigil_view/state_tests.rs"]
mod state_tests;
#[path = "sigil_view/view_tests.rs"]
mod view_tests;
