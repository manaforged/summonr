use std::cell::Cell;
use std::time::Duration;

use sigil::reactive::defer;
use sigil::tasks::on_message_or_close;

use tokio::task::JoinHandle;

use super::edit::{edit, edit_external, editor};
use super::intake::intake;
use super::*;

use crate::limits::BEAT;

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
    let version = info.version.clone();
    let slot: Rc<RefCell<Option<App>>> = Rc::default();
    let mounted = slot.clone();
    let commands = cmd_tx.clone();
    let closed = Rc::new(Cell::new(false));
    let inbox = RefCell::new(Some(ui_rx));
    let ended = closed.clone();
    let pump = Rc::new(RefCell::new(Pump {
        app: slot,
        closed,
        exit: None,
        session: String::new(),
        busy: None,
        asking: false,
        select: false,
    }));
    let inline = info.inline;
    let region = mounted.clone();
    let turn = pump.clone();
    let hooks = Hooks {
        external_events: Some(Box::new(move |events| turn.borrow_mut().turn(events))),
        after_frame: None,
        region_rows: Some(Box::new(move || {
            region.borrow().as_ref().map_or(8, inline_rows)
        })),
    };
    let config = sigil::AppConfig::new("summonr")
        .quit_on_ctrl_c(false)
        .terminal()
        .inline(inline)
        .terminal_hooks(hooks);
    let ran = {
        tokio::task::block_in_place(|| {
            sigil::run(config, move || {
                let app = App::new(info.clone(), workspace.clone(), commands.clone());
                if let Err(error) = calibrate(&mut std::io::stdout()) {
                    app.update(Msg::System(format!(
                        "Could not measure terminal text widths: {error}"
                    )));
                }
                if let Ok((cols, rows)) = crossterm::terminal::size() {
                    app.resize(cols, rows);
                }
                app.load();
                app.refresh();
                app.animate();
                if let Some(ui_rx) = inbox.borrow_mut().take() {
                    let receiver = app.clone();
                    let ended = ended.clone();
                    on_message_or_close(
                        ui_rx,
                        move |msg| {
                            receiver.update(msg);
                            let refresh = receiver.clone();
                            defer(move || refresh.refresh());
                        },
                        move || ended.set(true),
                    );
                }
                *mounted.borrow_mut() = Some(app.clone());
                root(app)
            })
        })
    };
    stop(worker, &cmd_tx, BEAT);
    ran.map_err(|error| Error::Tool(format!("the terminal failed: {error}")))?;
    let mut pump = pump.borrow_mut();
    let exit = pump.exit.take().unwrap_or(Exit::Code(0));
    if matches!(exit, Exit::Code(_)) {
        let session = &pump.session;
        eprintln!(
            "\n  summonr {version} · session {session}\n  resume: summonr -c · by id: summonr --session {session} · branch: summonr --fork\n"
        );
    }
    Ok(exit)
}

fn commit(app: &App) {
    if !app.inline {
        return;
    }
    let mut lines = String::new();
    app.model_mut(|model| lines = model.commit_lines());
    if !lines.is_empty() {
        drop(sigil::terminal::print_above(&lines));
    }
}

fn inline_rows(app: &App) -> u16 {
    let (cols, rows) = app.viewport.peek();
    let model = app.model.get();
    if app.splash.peek() || app.select.peek() || app.overlay().is_some() {
        return rows.max(1);
    }
    let mut height = 6u16;
    if model.busy_since.is_some() {
        height += 1;
    }
    height += crate::tui::sigil_view::chat::panels::strip_rows(app).min(9) as u16;
    height += crate::tui::sigil_view::chat::todos::height(app, cols);
    height.clamp(1, rows.max(1))
}

struct Pump {
    app: Rc<RefCell<Option<App>>>,
    closed: Rc<Cell<bool>>,
    exit: Option<Exit>,
    session: String,
    busy: Option<bool>,
    asking: bool,
    select: bool,
}

impl Pump {
    fn turn(&mut self, events: &mut Vec<ShellEvent>) {
        let Some(app) = self.app.borrow().clone() else {
            return;
        };
        if self.exit.is_some() {
            return;
        }
        intake(&app, events);
        if let Ok(size) = crossterm::terminal::size()
            && size != app.viewport.peek()
        {
            app.resize(size.0, size.1);
        }
        if let Some(id) = app.resume.borrow().clone() {
            return self.finish(&app, Exit::Resume(id));
        }
        if let Some(code) = status(self.closed.get(), app.quit.peek()) {
            return self.finish(&app, Exit::Code(code));
        }
        let busy = app.model.get().busy_since.is_some();
        let notify = app.model.get().prefs.notify;
        if self.busy != Some(busy) {
            drop(set_title(&title(&app, busy)));
            if self.busy == Some(true) && !busy && !focused() && notify.turns() {
                drop(bell(Some(&format!(
                    "summonr · {}: {}",
                    place(&app),
                    ending(&app)
                ))));
            }
            self.busy = Some(busy);
        }
        let asking = app.approval.peek().is_some();
        if !asking {
            self.asking = false;
        } else if !self.asking && !focused() && notify.approvals() {
            drop(bell(Some(&format!(
                "summonr · {}: approval needed",
                place(&app)
            ))));
            self.asking = true;
        }
        if !events.is_empty() {
            app.refresh();
        }
        let select = app.select.peek();
        if self.select != select {
            self.select = select;
            drop(set_mouse_capture(!select && !app.inline));
        }
        commit(&app);
        open_out(&app);
        copy_out(&app);
    }

    fn finish(&mut self, app: &App, exit: Exit) {
        self.session = app.model.get().session.to_string();
        self.exit = Some(exit);
        sigil::request_exit();
    }
}

fn open_out(app: &App) {
    if let Some(path) = app.open_file.peek() {
        app.open_file.set(None);
        if let Ok(Err(error)) | Err(error) = suspend(|| edit(&path, &editor())).map_err(Error::from)
        {
            app.push_entry(Entry::Error(error.to_string()));
        }
        app.refresh();
    }
    if app.editor_requested.peek() {
        app.editor_requested.set(false);
        let draft = app.draft.peek();
        match suspend(|| edit_external(&draft, &editor())).map_err(Error::from) {
            Ok(Ok(draft)) => app.edit(&draft),
            Ok(Err(error)) | Err(error) => {
                app.push_entry(Entry::Error(error.to_string()));
                app.refresh();
            }
        }
    }
}

pub(in crate::tui::sigil_view) fn copy_out(app: &App) {
    if let Some(text) = app.copy.peek() {
        app.copy.set(None);
        match sigil::clipboard::copy(&text) {
            Ok(delivery) => app.copied(&text, delivery),
            Err(error) => {
                app.push_entry(Entry::Error(format!("clipboard: {error}")));
                app.refresh();
            }
        }
    }
}

pub(in crate::tui::sigil_view) fn start_tick(tx: UnboundedSender<Cmd>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(BEAT);
        loop {
            interval.tick().await;
            if tx.send(Cmd::Tick).is_err() {
                break;
            }
        }
    });
}

pub(in crate::tui::sigil_view) fn stop<T>(
    worker: JoinHandle<T>,
    cmd: &UnboundedSender<Cmd>,
    wait: Duration,
) {
    drop(cmd.send(Cmd::Quit));
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return worker.abort();
    };
    tokio::task::block_in_place(|| {
        handle.block_on(async {
            let mut worker = worker;
            tokio::select! {
                _ = &mut worker => {}
                () = tokio::time::sleep(wait) => {
                    worker.abort();
                    drop(worker.await);
                }
            }
        });
    });
}

pub(in crate::tui::sigil_view) fn ending(app: &App) -> &'static str {
    let model = app.model.get();
    let last = model
        .entries
        .iter()
        .rev()
        .take_while(|entry| !matches!(entry, Entry::User(_)))
        .find(|entry| matches!(entry, Entry::Aborted(_) | Entry::Error(_)));
    match last {
        Some(Entry::Aborted(_)) => "turn cancelled",
        Some(_) => "turn failed",
        None => "turn finished",
    }
}

fn place(app: &App) -> String {
    app.workspace
        .file_name()
        .map(|name| {
            name.to_string_lossy()
                .chars()
                .filter(|ch| !ch.is_control())
                .collect()
        })
        .unwrap_or_default()
}

pub(in crate::tui::sigil_view) fn title(app: &App, busy: bool) -> String {
    let place = place(app);
    format!(
        "summon · {place} · {}",
        if busy { "working" } else { "ready" }
    )
}

pub(in crate::tui::sigil_view) fn status(closed: bool, quit: bool) -> Option<i32> {
    if quit {
        Some(0)
    } else if closed {
        Some(1)
    } else {
        None
    }
}
