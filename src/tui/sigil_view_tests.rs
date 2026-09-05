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

pub(super) fn info() -> Info {
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
        workflow: None,
        config: Some(scratch_config()),
    }
}

fn scratch_config() -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("mage-test-{}-{n}-config.toml", std::process::id()))
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

#[test]
fn a_run_layout_paints_the_ledger_beside_the_chat() {
    sigil::reactive::Runtime::scope("mage-layout-ledger-test", || {
        let (cmd, _rx) = unbounded_channel();
        let mut start = info();
        start.prefs.splash = false;
        start.prefs.layout = "run".to_string();
        let app = App::new(start, PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(120, 40);
        apply(
            &app,
            Msg::Event(crate::Event::Work {
                progress: crate::WorkProgress {
                    kind: crate::WorkKind::Steps,
                    objective: String::new(),
                    phase: crate::WorkPhase::Execute,
                    checks_failed: false,
                    current: Some(2),
                    total: 3,
                    attempts: 0,
                    rounds: 1,
                    max_turns: 0,
                    reason: None,
                    ledger: vec![
                        "I. Setup".to_string(),
                        "  [x] Make a.".to_string(),
                        "  [/] Make b.".to_string(),
                    ],
                    touched: Vec::new(),
                },
            }),
        );
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(120, 40);
        let text = screen
            .frame(&mut host, &mut router, [], (120, 40))
            .unwrap_or_else(|error| panic!("layout failed: {error}"))
            .text();
        let ledger_line = text
            .lines()
            .find(|line| line.contains("I. Setup"))
            .unwrap_or_else(|| panic!("no ledger line in:\n{text}"));
        let column = ledger_line.find("I. Setup").unwrap_or(0);
        assert!(
            column >= 71,
            "the ledger sits in the right pane, got column {column}"
        );
        assert!(text.contains("[/] Make b."), "{text}");
        assert!(text.contains("ledger"), "{text}");
        assert_eq!(app.chat_cols(), 71);
    });
}

#[test]
fn the_files_pane_walks_the_tree_and_attaches_a_file_to_the_prompt() {
    sigil::reactive::Runtime::scope("mage-files-pane-test", || {
        let (cmd, mut rx) = unbounded_channel();
        let mut start = info();
        start.prefs.splash = false;
        start.prefs.layout = "files".to_string();
        let app = App::new(start, PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(120, 40);
        apply(
            &app,
            Msg::Files(vec![
                "src/main.rs".to_string(),
                "src/lib.rs".to_string(),
                "README.md".to_string(),
            ]),
        );
        let names: Vec<String> = app.file_rows().iter().map(|row| row.name.clone()).collect();
        assert_eq!(names, ["README.md", "src"], "directories start collapsed");
        app.focus_pane(true);
        assert_eq!(app.pane_active(), Some(super::pane::Pane::Files));
        app.pane_move(1);
        app.pane_enter();
        let names: Vec<String> = app.file_rows().iter().map(|row| row.name.clone()).collect();
        assert_eq!(names, ["README.md", "src", "lib.rs", "main.rs"]);
        app.pane_move(1);
        app.pane_enter();
        assert_eq!(app.draft.peek(), "@src/lib.rs ");
        assert!(
            !app.pane_focus.peek(),
            "picking a file returns focus to the composer"
        );
        drop(rx.try_recv());
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(120, 40);
        let text = screen
            .frame(&mut host, &mut router, [], (120, 40))
            .unwrap_or_else(|error| panic!("layout failed: {error}"))
            .text();
        assert!(text.contains("files · mage-view-test"), "{text}");
        assert!(text.contains("▾ src"), "{text}");
        assert!(text.contains("main.rs"), "{text}");
    });
}

#[test]
fn pane_hotkeys_and_the_context_menu_drive_the_files_pane() {
    sigil::reactive::Runtime::scope("mage-files-hotkeys-test", || {
        let (cmd, mut rx) = unbounded_channel();
        let mut start = info();
        start.prefs.splash = false;
        let app = App::new(start, PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(120, 40);
        app.toggle_pane(super::pane::Pane::Files);
        assert_eq!(app.layout.peek().secondary, Some(super::pane::Pane::Files));
        assert!(app.pane_focus.peek(), "opening a pane focuses it");
        assert!(matches!(rx.try_recv(), Ok(Cmd::ListFiles)));
        apply(
            &app,
            Msg::Files(vec!["src/lib.rs".to_string(), "README.md".to_string()]),
        );
        app.pane_move(1);
        app.pane_expand();
        assert_eq!(app.file_rows().len(), 3);
        app.pane_move(1);
        app.pane_collapse();
        assert_eq!(
            app.file_cursor.peek(),
            1,
            "collapsing from a child lands on the parent when open"
        );
        app.pane_collapse();
        assert_eq!(app.file_rows().len(), 2, "a second ← collapses the parent");

        app.toggle_side();
        assert_eq!(app.layout.peek().secondary, None);
        app.toggle_side();
        assert_eq!(app.layout.peek().secondary, Some(super::pane::Pane::Files));

        app.pane_edge(false);
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(120, 40);
        screen
            .frame(&mut host, &mut router, [], (120, 40))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        let (col, row) = app
            .hits
            .find(&super::hit::Target::File(0))
            .unwrap_or_else(|| panic!("the first file row is a hit target"));
        assert!(
            app.context_at(col, row),
            "right-click on a file opens the menu"
        );
        let menu = app.context.peek().unwrap_or_else(|| panic!("menu"));
        assert_eq!(menu.items().len(), 4);
        app.context_move(1);
        app.context_move(-1);
        app.context_enter();
        assert_eq!(app.draft.peek(), "@README.md ");
        assert!(app.context.peek().is_none());
        let text = screen
            .frame(&mut host, &mut router, [], (120, 40))
            .unwrap_or_else(|error| panic!("layout failed: {error}"))
            .text();
        assert!(text.contains("README.md"), "{text}");
    });
}

#[test]
fn pane_header_close_button_hides_the_side_pane() {
    sigil::reactive::Runtime::scope("mage-pane-close-test", || {
        let (cmd, _rx) = unbounded_channel();
        let mut start = info();
        start.prefs.splash = false;
        let app = App::new(start, PathBuf::from("/tmp/mage-close-test"), cmd);
        app.resize(120, 40);
        app.toggle_pane(super::pane::Pane::Diff);

        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(120, 40);
        let text = screen
            .frame(&mut host, &mut router, [], (120, 40))
            .unwrap_or_else(|error| panic!("layout failed: {error}"))
            .text();
        assert!(
            text.contains('×'),
            "pane header has a close control:\n{text}"
        );

        let (col, row) = app
            .hits
            .find(&super::hit::Target::Close)
            .unwrap_or_else(|| panic!("close control is clickable"));
        app.press(col, row);
        assert_eq!(app.layout.peek().secondary, None);
    });
}

#[test]
fn resume_request_closes_the_ui_with_the_selected_session() {
    sigil::reactive::Runtime::scope("mage-resume-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-resume-test"), cmd);

        apply(&app, Msg::Resume("18d1584caf02c607".to_string()));

        assert_eq!(app.resume.borrow().as_deref(), Some("18d1584caf02c607"));
        assert!(app.quit.peek());
    });
}

#[test]
fn the_diff_pane_lists_changes_and_shows_hunks() {
    use crate::diff::{Diff, Stat};
    sigil::reactive::Runtime::scope("mage-diff-pane-test", || {
        let (cmd, mut rx) = unbounded_channel();
        let mut start = info();
        start.prefs.splash = false;
        let app = App::new(start, PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(120, 40);
        app.toggle_pane(super::pane::Pane::Diff);
        assert!(matches!(rx.try_recv(), Ok(Cmd::Diff(None))));
        apply(
            &app,
            Msg::Diff(Diff {
                base: "main".to_string(),
                files: vec![
                    Stat {
                        path: "src/lib.rs".to_string(),
                        added: 3,
                        removed: 1,
                        new: false,
                    },
                    Stat {
                        path: "new.txt".to_string(),
                        added: 2,
                        removed: 0,
                        new: true,
                    },
                ],
            }),
        );
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(120, 40);
        let text = screen
            .frame(&mut host, &mut router, [], (120, 40))
            .unwrap_or_else(|error| panic!("layout failed: {error}"))
            .text();
        assert!(text.contains("diff · 2 files · +5 −1 · vs main"), "{text}");
        assert!(text.contains("+3    −1    src/lib.rs"), "{text}");
        assert!(text.contains("new.txt (new)"), "{text}");

        app.pane_move(1);
        app.pane_enter();
        assert!(matches!(rx.try_recv(), Ok(Cmd::Patch(None, path)) if path == "new.txt"));
        apply(
            &app,
            Msg::Patch {
                path: "new.txt".to_string(),
                text: "--- /dev/null\n+++ b/new.txt\n@@ -0,0 +1,2 @@\n+new\n+file\n".to_string(),
            },
        );
        let text = screen
            .frame(&mut host, &mut router, [], (120, 40))
            .unwrap_or_else(|error| panic!("layout failed: {error}"))
            .text();
        assert!(text.contains("diff · new.txt · +2 −0"), "{text}");
        assert!(text.contains("@@ -0,0 +1,2 @@"), "{text}");
        assert!(text.contains("+file"), "{text}");
        assert!(app.pane_back(), "esc leaves the hunk view");
        assert!(app.diff_view.peek().is_none());

        screen
            .frame(&mut host, &mut router, [], (120, 40))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        let (col, row) = app
            .hits
            .find(&super::hit::Target::File(0))
            .unwrap_or_else(|| panic!("the first change row is a hit target"));
        assert!(app.context_at(col, row));
        let menu = app.context.peek().unwrap_or_else(|| panic!("menu"));
        assert_eq!(menu.items()[0].0, "view hunks");
        app.context_enter();
        assert!(matches!(rx.try_recv(), Ok(Cmd::Patch(None, path)) if path == "src/lib.rs"));
        assert_eq!(app.diff_view.peek().as_deref(), Some("src/lib.rs"));
    });
}

#[test]
fn the_worktrees_pane_lists_trees_and_opens_their_diff() {
    use crate::worktree::{Status, Tree};
    sigil::reactive::Runtime::scope("mage-worktrees-pane-test", || {
        let (cmd, mut rx) = unbounded_channel();
        let mut start = info();
        start.prefs.splash = false;
        let app = App::new(start, PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(120, 40);
        app.toggle_pane(super::pane::Pane::Worktrees);
        assert!(matches!(rx.try_recv(), Ok(Cmd::Trees)));
        let path = PathBuf::from("/tmp/mage-view-test-worktrees/abc");
        apply(
            &app,
            Msg::Trees(vec![Status {
                tree: Tree {
                    primary: PathBuf::from("/tmp/mage-view-test"),
                    path: path.clone(),
                    branch: "summon/abc".to_string(),
                    base: "main".to_string(),
                },
                ahead: 2,
                dirty: true,
                session: Some("0007".to_string()),
            }]),
        );
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(120, 40);
        let text = screen
            .frame(&mut host, &mut router, [], (120, 40))
            .unwrap_or_else(|error| panic!("layout failed: {error}"))
            .text();
        assert!(text.contains("worktrees · 1"), "{text}");
        assert!(
            text.contains("summon/abc       +2   dirty 7        abc"),
            "{text}"
        );

        app.pane_enter();
        assert_eq!(app.layout.peek().secondary, Some(super::pane::Pane::Diff));
        assert!(matches!(rx.try_recv(), Ok(Cmd::Diff(Some(root))) if root == path));
        let text = screen
            .frame(&mut host, &mut router, [], (120, 40))
            .unwrap_or_else(|error| panic!("layout failed: {error}"))
            .text();
        assert!(text.contains("diff · summon/abc"), "{text}");
        assert!(app.pane_back(), "esc returns to the worktree list");
        assert_eq!(
            app.layout.peek().secondary,
            Some(super::pane::Pane::Worktrees)
        );
        assert!(matches!(rx.try_recv(), Ok(Cmd::Trees)));

        screen
            .frame(&mut host, &mut router, [], (120, 40))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        let (col, row) = app
            .hits
            .find(&super::hit::Target::File(0))
            .unwrap_or_else(|| panic!("the worktree row is a hit target"));
        assert!(app.context_at(col, row));
        let menu = app.context.peek().unwrap_or_else(|| panic!("menu"));
        assert_eq!(menu.items()[1].0, "merge (ff-only)");
        app.context_move(1);
        app.context_enter();
        assert!(matches!(rx.try_recv(), Ok(Cmd::Merge(root)) if root == path));
    });
}

#[test]
fn the_view_pane_shows_a_file_with_its_added_lines() {
    use sigil_ide_core::buffer::Rope;
    sigil::reactive::Runtime::scope("mage-view-pane-test", || {
        let (cmd, mut rx) = unbounded_channel();
        let mut start = info();
        start.prefs.splash = false;
        let app = App::new(start, PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(120, 40);
        app.toggle_pane(super::pane::Pane::Files);
        assert!(matches!(rx.try_recv(), Ok(Cmd::ListFiles)));
        apply(&app, Msg::Files(vec!["README.md".to_string()]));
        app.pane_expand();
        assert_eq!(app.layout.peek().secondary, Some(super::pane::Pane::View));
        assert!(matches!(rx.try_recv(), Ok(Cmd::View(None, path)) if path == "README.md"));
        apply(
            &app,
            Msg::View {
                path: "README.md".to_string(),
                rope: Rope::from_str("one\ntwo\nthree\n"),
                added: vec![(2, 1)],
            },
        );
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(120, 40);
        let text = screen
            .frame(&mut host, &mut router, [], (120, 40))
            .unwrap_or_else(|error| panic!("layout failed: {error}"))
            .text();
        assert!(
            text.contains("view · README.md · 3 lines · +1 added"),
            "{text}"
        );
        assert!(text.contains("   1   one"), "{text}");
        assert!(text.contains("   2 + two"), "{text}");

        app.pane_move(2);
        let text = screen
            .frame(&mut host, &mut router, [], (120, 40))
            .unwrap_or_else(|error| panic!("layout failed: {error}"))
            .text();
        assert!(!text.contains("   1   one"), "scrolled past line 1: {text}");
        assert!(text.contains("   3   three"), "{text}");

        app.view_external();
        assert_eq!(app.open_file.peek().as_deref(), Some("README.md"));

        apply(&app, Msg::Changed(vec!["README.md".to_string()]));
        assert!(matches!(rx.try_recv(), Ok(Cmd::View(None, path)) if path == "README.md"));
        apply(&app, Msg::Changed(vec!["other.rs".to_string()]));
        assert!(
            rx.try_recv().is_err(),
            "an unrelated change does not reload the view"
        );

        assert!(app.pane_back(), "esc returns to the tree");
        assert_eq!(app.layout.peek().secondary, Some(super::pane::Pane::Files));
        assert!(matches!(rx.try_recv(), Ok(Cmd::ListFiles)));
        apply(&app, Msg::Changed(vec!["src/x.rs".to_string()]));
        assert!(
            matches!(rx.try_recv(), Ok(Cmd::ListFiles)),
            "a change relists the tree"
        );
    });
}

#[test]
fn the_diff_pane_splits_wide_hunks_filters_by_turn_and_copies_the_patch() {
    use crate::diff::{Diff, Stat};
    sigil::reactive::Runtime::scope("mage-diff-wide-test", || {
        let (cmd, mut rx) = unbounded_channel();
        let mut start = info();
        start.prefs.splash = false;
        let app = App::new(start, PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(220, 40);
        app.set_layout(super::pane::Layout::parse("chat | diff 60%").unwrap_or_default());
        assert!(matches!(rx.try_recv(), Ok(Cmd::Diff(None))));
        app.focus_pane(true);
        let stat = |path: &str| Stat {
            path: path.to_string(),
            added: 1,
            removed: 1,
            new: false,
        };
        apply(
            &app,
            Msg::Diff(Diff {
                base: "HEAD".to_string(),
                files: vec![stat("a.rs"), stat("b.rs")],
            }),
        );
        apply(&app, Msg::Edited(vec!["b.rs".to_string()]));
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(220, 40);
        let frame = |screen: &mut Screen, host: &mut Host, router: &mut EventRouter| {
            screen
                .frame(host, router, [], (220, 40))
                .unwrap_or_else(|error| panic!("layout failed: {error}"))
                .text()
        };
        let text = frame(&mut screen, &mut host, &mut router);
        assert!(text.contains("a.rs") && text.contains("b.rs"), "{text}");

        app.toggle_recent();
        let text = frame(&mut screen, &mut host, &mut router);
        assert!(text.contains("· last turn"), "{text}");
        assert!(
            !text.contains("a.rs"),
            "the filter hides files the turn did not edit: {text}"
        );
        assert!(text.contains("b.rs"), "{text}");

        app.pane_enter();
        assert!(matches!(rx.try_recv(), Ok(Cmd::Patch(None, path)) if path == "b.rs"));
        apply(
            &app,
            Msg::Patch {
                path: "b.rs".to_string(),
                text: "@@ -1,2 +1,2 @@\n keep\n-old line\n+new line\n".to_string(),
            },
        );
        let text = frame(&mut screen, &mut host, &mut router);
        let side = text
            .lines()
            .find(|line| line.contains("old line"))
            .unwrap_or_else(|| panic!("the removed line is on screen: {text}"));
        assert!(
            side.contains("│ new line"),
            "old and new share a row: {side}"
        );
        assert!(text.contains("keep"), "{text}");

        app.copy_patch();
        assert_eq!(
            app.copy.peek().as_deref(),
            Some("@@ -1,2 +1,2 @@\n keep\n-old line\n+new line\n")
        );
        app.copy.set(None);

        assert!(app.pane_back());
        app.copy_patch();
        assert!(matches!(rx.try_recv(), Ok(Cmd::Patch(None, path)) if path.is_empty()));
        apply(
            &app,
            Msg::Patch {
                path: String::new(),
                text: "whole".to_string(),
            },
        );
        assert_eq!(app.copy.peek().as_deref(), Some("whole"));
    });
}

#[test]
fn the_view_pane_edits_a_file_in_place_and_saves_it() {
    use sigil_ide_core::buffer::Rope;
    sigil::reactive::Runtime::scope("mage-edit-pane-test", || {
        let (cmd, mut rx) = unbounded_channel();
        let mut start = info();
        start.prefs.splash = false;
        let app = App::new(start, PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(120, 40);
        app.view_open("notes.md".to_string());
        assert!(matches!(rx.try_recv(), Ok(Cmd::View(None, path)) if path == "notes.md"));
        apply(
            &app,
            Msg::View {
                path: "notes.md".to_string(),
                rope: Rope::from_str("one\ntwo\n"),
                added: Vec::new(),
            },
        );
        assert_eq!(app.pane_active(), Some(super::pane::Pane::View));
        app.edit_start();
        assert!(app.view_edit.peek());
        assert_eq!(app.view_text.peek(), "one\ntwo\n");
        assert_eq!(
            app.pane_active(),
            None,
            "keys go to the editor while editing"
        );

        app.view_caret.set(3);
        app.submit();
        assert_eq!(
            app.view_text.peek(),
            "one\n\ntwo\n",
            "enter inserts a newline"
        );
        assert_eq!(app.view_caret.peek(), 4);

        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(120, 40);
        let text = screen
            .frame(&mut host, &mut router, [], (120, 40))
            .unwrap_or_else(|error| panic!("layout failed: {error}"))
            .text();
        assert!(text.contains("edit · notes.md · 3 lines"), "{text}");
        assert!(
            text.lines().any(|line| line.trim_end().ends_with("│    2")),
            "the inserted blank line is numbered: {text}"
        );
        assert!(
            text.contains("editing notes.md · ctrl+s save · esc leave"),
            "{text}"
        );

        assert!(
            app.edit_stop(),
            "first esc with unsaved text arms the discard"
        );
        assert!(app.view_edit.peek(), "still editing");
        app.edit_save();
        assert!(
            matches!(rx.try_recv(), Ok(Cmd::Save(path, text)) if path == "notes.md" && text == "one\n\ntwo\n")
        );
        assert!(app.edit_stop(), "esc after a save leaves");
        assert!(!app.view_edit.peek());
        assert_eq!(app.pane_active(), Some(super::pane::Pane::View));

        app.edit_start();
        app.view_text.set("changed".to_string());
        app.toggle_pane(super::pane::Pane::Files);
        assert_eq!(
            app.layout.peek().secondary,
            Some(super::pane::Pane::View),
            "unsaved edits keep the pane"
        );
        assert!(app.edit_stop() && app.edit_stop(), "two escapes discard");
        assert!(!app.view_edit.peek());
    });
}

#[test]
fn a_multi_line_paste_lands_in_the_draft_at_the_caret() {
    sigil::reactive::Runtime::scope("mage-paste-test", || {
        let (cmd, _rx) = unbounded_channel();
        let mut start = info();
        start.prefs.splash = false;
        let app = App::new(start, PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(120, 40);
        app.replace("fix ".to_string());
        app.insert("line one\r\nline two\n");
        assert_eq!(app.draft.peek(), "fix line one\nline two\n");
        assert_eq!(app.caret.peek(), "fix line one\nline two\n".len());
        app.caret.set(0);
        app.insert("> ");
        assert_eq!(app.draft.peek(), "> fix line one\nline two\n");
        assert_eq!(app.caret.peek(), 2);
    });
}
