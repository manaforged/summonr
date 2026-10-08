use std::cell::RefCell;
use std::path::{Path, PathBuf};

use sigil::prelude::effect;
use sigil::reactive::defer;
use sigil::reactive::untrack;
use sigil::tasks::on_message;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use super::app::App;
use super::desktop::root;
use super::host::{start_tick, stop};
use super::model::Entry;
use crate::error::{Error, Result};
use crate::provider::Client;
use crate::runtime::Loop;
use crate::tui::hub::{Factory, Started, start};
use crate::tui::{Info, Msg};

const TITLE: &str = "Summonr";
const SIZE: (u32, u32) = (1280, 800);

pub(crate) fn run(
    loop_: Loop<Client>,
    workspace: PathBuf,
    info: Info,
    ui_tx: UnboundedSender<Msg>,
    ui_rx: UnboundedReceiver<Msg>,
    factory: Factory,
) -> Result<i32> {
    let Started {
        commands: cmd_tx,
        inbox: ui_rx,
        task: worker,
    } = start(loop_, info.clone(), (ui_tx, ui_rx), factory);
    start_tick(cmd_tx.clone());
    let inbox = RefCell::new(Some(ui_rx));
    let commands = cmd_tx.clone();
    let opened = tokio::task::block_in_place(|| {
        sigil::run(window(), move || {
            super::canvas::window_cells();
            let app = App::new(info.clone(), workspace.clone(), commands.clone());
            app.resize(160, 50);
            app.refresh();
            app.animate();
            if let Some(ui_rx) = inbox.borrow_mut().take() {
                let receiver = app.clone();
                on_message(ui_rx, move |msg| {
                    untrack(|| {
                        receiver.update(msg);
                        receiver.refresh();
                    });
                });
            }
            watch_outputs(&app);
            root(app)
        })
    });
    stop(worker, &cmd_tx, crate::limits::limits().worker_stop());
    opened.map_err(|error| Error::Tool(format!("the window failed: {error}")))?;
    Ok(0)
}

fn window() -> sigil::AppConfig {
    sigil::AppConfig::new(TITLE)
        .size(SIZE.0, SIZE.1)
        .esc_exits(false)
}

fn watch_outputs(app: &App) {
    let app = app.clone();
    effect(move || {
        let pending = app.open_file.get().is_some()
            || app.copy.get().is_some()
            || app.editor_requested.get()
            || app.quit.get();
        if pending {
            let handler = app.clone();
            defer(move || outputs(&handler));
        }
    });
}

fn outputs(app: &App) {
    if let Some(path) = app.open_file.peek() {
        app.open_file.set(None);
        if let Err(error) = reveal(&path) {
            app.push_entry(Entry::Error(error.to_string()));
        }
        app.refresh();
    }
    app.copy_out();
    if app.editor_requested.peek() {
        app.editor_requested.set(false);
        app.model_mut(|model| {
            let key = model.prefs.hotkeys.label(crate::prefs::Hotkey::Editor);
            model.entries.push(Entry::System(format!(
                "{key} opens $EDITOR in the terminal interface · the window edits in place"
            )))
        });
        app.refresh();
    }
    if app.quit.peek() {
        sigil::request_exit();
    }
}

fn reveal(path: &Path) -> Result<()> {
    summonr_auth::open::external(path).map_err(|error| Error::Tool(error.to_string()))
}
