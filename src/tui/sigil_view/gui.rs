use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use sigil::prelude::effect;
use sigil::reactive::defer;
use sigil::reactive::untrack;
use sigil::tasks::on_message;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use super::app::App;
use super::desktop::root;
use super::host::{copy_out, start_tick, stop};
use super::model::Entry;
use crate::error::{Error, Result};
use crate::provider::Client;
use crate::runtime::Loop;
use crate::tui::{Exit, Info, Msg, worker};

const TITLE: &str = "Summonr";
const SIZE: (u32, u32) = (1280, 800);

pub(crate) fn run(
    loop_: Loop<Client>,
    workspace: PathBuf,
    info: Info,
    ui_tx: UnboundedSender<Msg>,
    ui_rx: UnboundedReceiver<Msg>,
) -> Result<Exit> {
    let (cmd_tx, cmd_rx) = unbounded_channel();
    start_tick(cmd_tx.clone());
    let worker = tokio::spawn(worker(loop_, cmd_rx, ui_tx, info.clone()));
    let inbox = RefCell::new(Some(ui_rx));
    let commands = cmd_tx.clone();
    let resume = Rc::new(RefCell::new(None));
    let requested = resume.clone();
    let opened = tokio::task::block_in_place(|| {
        sigil::run(window(), move || {
            super::canvas::window_cells();
            let mut app = App::new(info.clone(), workspace.clone(), commands.clone());
            app.resume = requested.clone();
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
    stop(worker, &cmd_tx, crate::limits::WORKER_STOP);
    opened.map_err(|error| Error::Tool(format!("the window failed: {error}")))?;
    let selected = resume.borrow().clone();
    Ok(selected.map_or(Exit::Code(0), Exit::Resume))
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
    copy_out(app);
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
