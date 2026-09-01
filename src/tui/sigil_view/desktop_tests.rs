use std::path::PathBuf;

use sigil::testing::with_cx;
use tokio::sync::mpsc::unbounded_channel;

use super::root;
use crate::diff::{Diff, Stat};
use crate::tui::sigil_view::app::App;
use crate::tui::sigil_view::pane::Pane;
use crate::tui::{Cmd, Msg};

fn app() -> (App, tokio::sync::mpsc::UnboundedReceiver<Cmd>) {
    let (cmd, rx) = unbounded_channel();
    let mut info = super::super::tests::info();
    info.prefs.splash = false;
    let app = App::new(info, PathBuf::from("/tmp/mage-desktop-test"), cmd);
    app.resize(160, 50);
    (app, rx)
}

#[test]
fn the_window_tree_builds_and_asks_for_the_diff() {
    with_cx("summoner-desktop-tree", |cx| {
        let (app, mut rx) = app();
        let node = cx.add(root(app));
        assert!(cx.first_child(node).is_some(), "rail and main column");
        assert!(matches!(rx.try_recv(), Ok(Cmd::Diff(None))));
    });
}

#[test]
fn the_right_rail_follows_the_open_pane() {
    with_cx("summoner-desktop-panes", |cx| {
        let (app, mut rx) = app();
        app.update(Msg::Diff(Diff {
            base: "HEAD".to_string(),
            files: vec![Stat {
                path: "src/lib.rs".to_string(),
                added: 2,
                removed: 1,
                new: false,
            }],
        }));
        app.refresh();
        for pane in [Pane::Diff, Pane::Files, Pane::Worktrees, Pane::Ledger] {
            app.toggle_pane(pane);
            assert_eq!(app.layout.peek().secondary, Some(pane));
            let node = cx.add(root(app.clone()));
            assert!(cx.first_child(node).is_some(), "{pane:?} pane builds");
            app.toggle_pane(pane);
        }
        app.toggle_pane(Pane::Diff);
        app.diff_open("src/lib.rs".to_string());
        app.update(Msg::Patch {
            path: "src/lib.rs".to_string(),
            text: "@@ -1 +1,2 @@\n-a\n+b\n+c\n".to_string(),
        });
        app.refresh();
        let node = cx.add(root(app.clone()));
        assert!(cx.first_child(node).is_some(), "hunk view builds");
        while rx.try_recv().is_ok() {}
    });
}

#[test]
fn options_and_hotkey_editor_build_in_the_desktop_rail() {
    with_cx("summoner-desktop-options", |cx| {
        let (app, _rx) = app();
        app.toggle_options();
        assert!(app.settings_open.peek());
        let options = cx.add(root(app.clone()));
        assert!(cx.first_child(options).is_some(), "options tree builds");

        app.settings_sel.set(12);
        app.adjust_setting(1);
        assert!(app.hotkeys_open.peek());
        let hotkeys = cx.add(root(app.clone()));
        assert!(cx.first_child(hotkeys).is_some(), "hotkey tree builds");

        app.edit_hotkey();
        assert!(app.route_hotkey('k'));
        assert_eq!(
            app.model
                .borrow()
                .prefs
                .hotkeys
                .label(crate::prefs::Hotkey::Options),
            "ctrl+k"
        );
    });
}
