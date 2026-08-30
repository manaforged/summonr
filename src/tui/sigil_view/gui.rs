use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use sigil::prelude::Cx;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use super::super::{Cmd, Info, Msg, worker};
use super::app::App;
use super::desktop::root;
use super::host::start_tick;
use super::model::Entry;
use crate::error::{Error, Result};
use crate::provider::Client;
use crate::runtime::Loop;

const TITLE: &str = "Summoner";
const SIZE: (u32, u32) = (1280, 800);
const FRAME: Duration = Duration::from_millis(40);

type Inbox = Rc<RefCell<UnboundedReceiver<Msg>>>;

/// Runs the same panes as the terminal host in a desktop window. Returns
/// when the window closes.
pub(crate) fn run(
    loop_: Loop<Client>,
    workspace: PathBuf,
    info: Info,
    ui_tx: UnboundedSender<Msg>,
    ui_rx: UnboundedReceiver<Msg>,
) -> Result<i32> {
    let (cmd_tx, cmd_rx) = unbounded_channel();
    start_tick(cmd_tx.clone());
    let worker = tokio::spawn(worker(loop_, cmd_rx, ui_tx, info.clone()));
    let inbox: Inbox = Rc::new(RefCell::new(ui_rx));
    let commands = cmd_tx.clone();
    let opened = tokio::task::block_in_place(|| {
        sigil::launch(window(), move |cx: &mut Cx| {
            let app = App::new(info, workspace, commands);
            app.resize(160, 50);
            app.refresh();
            let tick = app.clone();
            let inbox = inbox.clone();
            let status_at = Cell::new(Instant::now());
            let _interval = cx.interval(FRAME, move || pump(&tick, &inbox, &status_at));
            cx.add(root(app))
        })
    });
    drop(cmd_tx.send(Cmd::Quit));
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async {
            let mut worker = worker;
            tokio::select! {
                _ = &mut worker => {}
                () = tokio::time::sleep(Duration::from_secs(2)) => {
                    worker.abort();
                    drop(worker.await);
                }
            }
        });
    });
    opened
        .map(|()| 0)
        .map_err(|error| Error::Tool(format!("the window failed: {error}")))
}

fn window() -> sigil::AppConfig {
    sigil::AppConfig::from((TITLE, SIZE.0, SIZE.1)).esc_exits(false)
}

/// One frame of housekeeping: drain the worker's messages, tick, and act on
/// requests the terminal host serves by suspending the screen.
fn pump(app: &App, inbox: &Inbox, status_at: &Cell<Instant>) {
    let mut changed = false;
    while let Ok(msg) = inbox.borrow_mut().try_recv() {
        app.update(msg);
        changed = true;
    }
    if changed {
        app.refresh();
    } else if app.model.borrow().busy_since.is_some()
        && status_at.get().elapsed() >= Duration::from_secs(1)
    {
        app.refresh_status();
        status_at.set(Instant::now());
    }
    app.tick();
    if let Some(path) = app.open_file.peek() {
        app.open_file.set(None);
        if let Err(error) = reveal(&app.workspace.join(&path)) {
            app.model
                .borrow_mut()
                .entries
                .push(Entry::Error(error.to_string()));
        }
        app.refresh();
    }
    if let Some(text) = app.copy.peek() {
        app.copy.set(None);
        match sigil::clipboard::copy(&text) {
            Ok(()) => app.copied(&text),
            Err(error) => {
                app.model
                    .borrow_mut()
                    .entries
                    .push(Entry::Error(format!("clipboard: {error}")));
                app.refresh();
            }
        }
    }
    if app.editor_requested.peek() {
        app.editor_requested.set(false);
        app.model.borrow_mut().entries.push(Entry::System(
            "ctrl+g opens $EDITOR in the terminal interface; the window edits in place".to_string(),
        ));
        app.refresh();
    }
    if app.quit.peek() {
        // ponytail: the process ends here; upgrade when sigil exposes an exit request.
        std::process::exit(0);
    }
}

/// Opens `path` with the desktop's default application.
fn reveal(path: &Path) -> Result<()> {
    let (program, args): (&str, Vec<&str>) = if cfg!(target_os = "macos") {
        ("open", vec![])
    } else if cfg!(windows) {
        ("cmd", vec!["/C", "start", ""])
    } else {
        ("xdg-open", vec![])
    };
    let status = std::process::Command::new(program)
        .args(args)
        .arg(path)
        .status()
        .map_err(|error| Error::Tool(format!("cannot start {program}: {error}")))?;
    if !status.success() {
        return Err(Error::Tool(format!("{program} exited with {status}")));
    }
    Ok(())
}
