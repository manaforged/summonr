use std::cell::Cell;
use std::time::Duration;

use sigil::reactive::defer;
use sigil::tasks::on_message_or_close;

use tokio::task::JoinHandle;

use super::edit::{edit, edit_external, editor};
use super::*;

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
            region.borrow().as_ref().map_or(8, App::inline_rows)
        })),
    };
    let config = sigil::AppConfig::new("summonr")
        .quit_on_ctrl_c(false)
        .terminal()
        .inline(inline)
        .modify_other_keys(true)
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
                    let dirty = Rc::new(Cell::new(false));
                    on_message_or_close(
                        ui_rx,
                        move |msg| {
                            receiver.update(msg);
                            if !dirty.replace(true) {
                                let refresh = receiver.clone();
                                let dirty = dirty.clone();
                                defer(move || {
                                    dirty.set(false);
                                    refresh.refresh();
                                });
                            }
                        },
                        move || ended.set(true),
                    );
                }
                *mounted.borrow_mut() = Some(app.clone());
                root(app)
            })
        })
    };
    stop(worker, &cmd_tx, crate::limits::limits().beat());
    ran.map_err(|error| Error::Tool(format!("the terminal failed: {error}")))?;
    let mut pump = pump.borrow_mut();
    let exit = pump.exit.take().unwrap_or(0);
    let session = crate::session::short(&pump.session);
    eprintln!(
        "\n  summonr {version} · session {session}\n  resume: summonr -c · by id: summonr --session {session} · branch: summonr --fork\n"
    );
    Ok(exit)
}

impl App {
    fn commit(&self) {
        if !self.inline {
            return;
        }
        let mut lines = String::new();
        self.model_mut(|model| lines = model.commit_lines());
        if !lines.is_empty() {
            drop(sigil::terminal::print_above(&lines));
        }
    }

    fn inline_rows(&self) -> u16 {
        let (cols, rows) = self.viewport.peek();
        let model = self.model.get();
        if self.splash.peek() || self.select.peek() || self.overlay().is_some() {
            return rows.max(1);
        }
        let mut height = 6u16;
        if model.busy_since.is_some() {
            height += 1;
        }
        height += crate::tui::sigil_view::chat::panels::strip_rows(self).min(9) as u16;
        height += crate::tui::sigil_view::chat::todos::height(self, cols);
        height.clamp(1, rows.max(1))
    }

    fn open_out(&self) {
        if let Some(path) = self.open_file.peek() {
            self.open_file.set(None);
            if let Ok(Err(error)) | Err(error) =
                suspend(|| edit(&path, &editor())).map_err(Error::from)
            {
                self.push_entry(Entry::Error(error.to_string()));
            }
            self.refresh();
        }
        if self.editor_requested.peek() {
            self.editor_requested.set(false);
            let draft = self.draft.peek();
            match suspend(|| edit_external(&draft, &editor())).map_err(Error::from) {
                Ok(Ok(draft)) => self.edit(&draft),
                Ok(Err(error)) | Err(error) => {
                    self.push_entry(Entry::Error(error.to_string()));
                    self.refresh();
                }
            }
        }
    }

    pub(in crate::tui::sigil_view) fn copy_out(&self) {
        if let Some(text) = self.copy.peek() {
            self.copy.set(None);
            match sigil::clipboard::copy(&text) {
                Ok(delivery) => self.copied(&text, delivery),
                Err(error) => {
                    self.push_entry(Entry::Error(format!("clipboard: {error}")));
                    self.refresh();
                }
            }
        }
    }

    pub(in crate::tui::sigil_view) fn ending(&self) -> &'static str {
        let model = self.model.get();
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

    fn place(&self) -> String {
        self.workspace
            .file_name()
            .map(|name| {
                name.to_string_lossy()
                    .chars()
                    .filter(|ch| !ch.is_control())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(in crate::tui::sigil_view) fn title(&self, busy: bool) -> String {
        let place = self.place();
        format!(
            "summon · {place} · {}",
            if busy { "working" } else { "ready" }
        )
    }
}

struct Pump {
    app: Rc<RefCell<Option<App>>>,
    closed: Rc<Cell<bool>>,
    exit: Option<i32>,
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
        app.intake(events);
        if let Ok(size) = crossterm::terminal::size()
            && size != app.viewport.peek()
        {
            app.resize(size.0, size.1);
        }
        if let Some(code) = status(self.closed.get(), app.quit.peek()) {
            return self.finish(&app, code);
        }
        let busy = app.model.get().busy_since.is_some();
        let notify = app.model.get().prefs.notify;
        self.track_busy(&app, busy, notify);
        self.track_asking(&app, notify);
        if !events.is_empty() {
            app.refresh();
        }
        let select = app.select.peek();
        if self.select != select {
            self.select = select;
            drop(set_mouse_capture(!select && !app.inline));
        }
        app.commit();
        app.open_out();
        app.copy_out();
    }

    fn track_busy(&mut self, app: &App, busy: bool, notify: crate::prefs::Notify) {
        if self.busy == Some(busy) {
            return;
        }
        drop(set_title(&app.title(busy)));
        if self.busy == Some(true) && !busy && !focused() && notify.turns() {
            drop(bell(Some(&format!(
                "summonr · {}: {}",
                app.place(),
                app.ending()
            ))));
        }
        self.busy = Some(busy);
    }

    fn track_asking(&mut self, app: &App, notify: crate::prefs::Notify) {
        let asking = app.approval.peek().is_some();
        if !asking {
            self.asking = false;
        } else if !self.asking && !focused() && notify.approvals() {
            drop(bell(Some(&format!(
                "summonr · {}: approval needed",
                app.place()
            ))));
            self.asking = true;
        }
    }

    fn finish(&mut self, app: &App, exit: i32) {
        self.session = app.model.get().session.to_string();
        self.exit = Some(exit);
        sigil::request_exit();
    }
}

pub(in crate::tui::sigil_view) fn start_tick(tx: UnboundedSender<Cmd>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(crate::limits::limits().beat());
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

pub(in crate::tui::sigil_view) fn status(closed: bool, quit: bool) -> Option<i32> {
    if quit {
        Some(0)
    } else if closed {
        Some(1)
    } else {
        None
    }
}
