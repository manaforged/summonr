use sigil::prelude::signal;
use sigil::reactive::{Owner, effect, untrack};
use tokio::sync::mpsc::unbounded_channel;

use super::super::App;
use super::arrange::{along, refuse, room};
use super::{Pane, changed, focus, with};
use summonr_agent::hub::{Cmd, Open, View};
use summonr_agent::prefs::Axis;

pub(super) fn add(axis: Axis, open: impl FnOnce(std::path::PathBuf) -> Open) -> Option<App> {
    let (main, workspace, root, at, extent) = with(|split| {
        let at = split.focus.peek().min(split.panes.len() - 1);
        let pane = &split.panes[at];
        let extent = along(pane.bounds.peek(), axis);
        (
            split.panes[0].app.clone(),
            pane.app.workspace(),
            split.root,
            at,
            extent,
        )
    })?;
    if !room(extent, axis) {
        refuse(&main, axis);
        return None;
    }
    let owner = Owner::new("split-pane", root);
    let (app, bounds) = owner.with(|| (attach(&main), signal((0.0, 0.0, 0.0, 0.0))));
    app.open_session(open(workspace));
    let opened = app.clone();
    let new = with(|split| {
        split.panes.push(Pane {
            app,
            owner: Some(owner),
            bounds,
        });
        let new = split.panes.len() - 1;
        split.shape.split(at, axis, new);
        split.zoom = None;
        new
    })?;
    changed();
    focus(new);
    Some(opened)
}

fn attach(main: &App) -> App {
    let (commands, command_rx) = unbounded_channel();
    let (ui, inbox) = unbounded_channel();
    drop(main.cmd.send(Cmd::Attach(View {
        commands: command_rx,
        ui,
    })));
    let mut app = App::new(std::rc::Rc::clone(&main.info), main.workspace(), commands);
    app.window.share(&main.window);
    app.settings_open = main.settings_open;
    app.settings_group = main.settings_group;
    let size = main.viewport;
    let fit = app.clone();
    effect(move || {
        let (cols, rows) = size.get();
        fit.resize(cols, rows);
    });
    let (watch, list) = (app.clone(), main.cmd.clone());
    let session = sigil::reactive::derived(move || watch.model.with(|model| model.session.clone()));
    effect(move || {
        session.with(|_| ());
        drop(list.send(Cmd::ListSessions));
    });
    app.load();
    app.refresh();
    app.animate();
    let receiver = app.clone();
    sigil::tasks::on_message(
        inbox,
        move |msg| {
            untrack(|| {
                receiver.update(msg);
                receiver.refresh();
            });
        },
        || {},
    );
    app
}
