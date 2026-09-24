use super::edit::{edit, edit_external, editor};

use super::intake::{ANIMATION, finish_intake, frame, read_event};
use super::terminal::{Paste, Terminal};
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
    let (exit, session) = sigil::reactive::Runtime::scope("summonr-tui", || {
        let app = App::new(info, workspace, cmd_tx.clone());
        let exit = live(app.clone(), ui_rx);
        let session = app.model.get().session.clone();
        exit.map(|exit| (exit, session))
    })?;
    drop(cmd_tx.send(Cmd::Quit));
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => {
            tokio::task::block_in_place(|| {
                handle.block_on(async {
                    let mut worker = worker;
                    tokio::select! {
                        _ = &mut worker => {}
                        () = tokio::time::sleep(BEAT) => {
                            worker.abort();
                            drop(worker.await);
                        }
                    }
                });
            });
        }
        Err(_) => worker.abort(),
    }
    if matches!(exit, Exit::Code(_)) {
        eprintln!(
            "\n  summonr {version} · session {session}\n  resume: summonr -c · by id: summonr --session {session} · branch: summonr --fork\n"
        );
    }
    Ok(exit)
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

pub(in crate::tui::sigil_view) fn live(app: App, mut ui: UnboundedReceiver<Msg>) -> Result<Exit> {
    guard();
    let mut term = Terminal::enter()?;
    if let Err(error) = calibrate(&mut term.out) {
        app.update(Msg::System(format!(
            "Could not measure terminal text widths: {error}"
        )));
    }
    install(Paste {
        text: term.paste.clone(),
        copy: app.copy,
    });
    let (mut cols, mut rows) = crossterm::terminal::size()?;
    app.resize(cols, rows);
    app.load();
    let mut host = Host::cells();
    app.refresh();
    let mut router = EventRouter::new();
    let tree = app.clone();
    host.mount(move |cx: &mut Cx| cx.add(root(tree)));
    let mut screen = Screen::new(cols, rows);
    frame(&mut screen, &mut host, &mut router, [], (cols, rows))?;
    router.focus_next(host.cx());
    term.paint(&mut screen)?;
    let mut pointer = Point::new(0.0, 0.0);
    let mut selecting = false;
    let mut status_at = Instant::now();
    let mut animation_at = Instant::now();
    let mut was_busy = None;
    term.title(&title(&app, false))?;
    let mut framed = Instant::now();
    loop {
        let mut incoming = Vec::new();
        let busy = app.model.get().busy_since.is_some();
        let splash_on =
            app.splash.peek() && (app.model.get().prefs.animations || !app.splash_revealed.peek());
        let budget = frame_budget(app.model.get().prefs.fps);
        let mut wait = if busy || splash_on {
            budget
        } else {
            crate::limits::IDLE_POLL
        };
        while event::poll(wait)? {
            let mut staged = false;
            let mut motion = true;
            loop {
                let next = event::read()?;
                motion &=
                    matches!(&next, TermEvent::Mouse(mouse) if mouse.kind == MouseEventKind::Moved);
                staged |= read_event(
                    next,
                    &mut incoming,
                    &mut pointer,
                    &mut screen,
                    &mut term,
                    &app,
                    staged,
                )?;
                if !event::poll(Duration::ZERO)? {
                    break;
                }
            }
            finish_intake(&mut incoming, staged, &mut term);
            (cols, rows) = app.viewport.peek();
            wait = budget.saturating_sub(framed.elapsed());
            if !motion || wait.is_zero() {
                break;
            }
        }
        let mut dirty = !incoming.is_empty() || term.redraw;
        while let Ok(msg) = ui.try_recv() {
            app.update(msg);
            dirty = true;
        }
        if let Some(id) = app.resume.borrow().clone() {
            return Ok(Exit::Resume(id));
        }
        if let Some(code) = status(ui.is_closed(), app.quit.peek()) {
            return Ok(Exit::Code(code));
        }
        let busy = app.model.get().busy_since.is_some();
        if was_busy != Some(busy) {
            term.title(&title(&app, busy))?;
            if was_busy == Some(true) && !busy && !term.focused {
                term.bell()?;
            }
            was_busy = Some(busy);
        }
        if dirty {
            app.refresh();
        } else if app.model.get().busy_since.is_some()
            && status_at.elapsed() >= crate::limits::STATUS_REFRESH
        {
            app.refresh_status();
            dirty = true;
            status_at = Instant::now();
        }
        if animation_at.elapsed() >= ANIMATION {
            animation_at = Instant::now();
            app.tick();
            dirty |= busy || splash_on;
        }
        dirty |= app.open_file.peek().is_some()
            || app.copy.peek().is_some()
            || app.editor_requested.peek()
            || app.selection_anchor.peek().is_some();
        if dirty {
            let grid = frame(&mut screen, &mut host, &mut router, incoming, (cols, rows))?;
            framed = Instant::now();
            if let Some((col, row)) = app.selection_anchor.peek() {
                app.selection_anchor.set(None);
                term.anchor = Some((col, row));
            }
            if let Some((col, row)) = term.anchor.take() {
                term.selection = Some(Selection::new(grid.clone(), app.chat_cols(), col, row));
            }
            let next = app.select.peek();
            if next != selecting {
                term.select(next)?;
                selecting = next;
            }
            if let Some(path) = app.open_file.peek() {
                app.open_file.set(None);
                term.suspend()?;
                let opened = edit(&path, &editor());
                term.resume(selecting)?;
                screen.invalidate();
                if let Err(error) = opened {
                    app.push_entry(Entry::Error(error.to_string()));
                }
                app.refresh();
            }
            if let Some(text) = app.copy.peek() {
                app.copy.set(None);
                app.copied(&text, term.copy(&text)?);
            }
            if app.editor_requested.peek() {
                app.editor_requested.set(false);
                term.suspend()?;
                let edited = edit_external(&app.draft.peek(), &editor());
                term.resume(selecting)?;
                screen.invalidate();
                match edited {
                    Ok(draft) => app.edit(&draft),
                    Err(error) => {
                        app.push_entry(Entry::Error(error.to_string()));
                        app.refresh();
                    }
                }
            }
            term.paint(&mut screen)?;
        }
    }
}

pub(in crate::tui::sigil_view) fn title(app: &App, busy: bool) -> String {
    let place = app
        .workspace
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
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

pub(in crate::tui::sigil_view) fn guard() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            drop(execute!(
                std::io::stdout(),
                Print("\x1b[?2026l"),
                DisableBracketedPaste,
                DisableFocusChange,
                DisableMouseCapture,
                SetTitle(""),
                cursor::Show,
                LeaveAlternateScreen
            ));
            drop(disable_raw_mode());
            previous(info);
        }));
    });
}

pub(in crate::tui::sigil_view) fn pbcopy(text: &str) -> bool {
    if !cfg!(target_os = "macos") {
        return false;
    }
    let Ok(mut child) = std::process::Command::new("/usr/bin/pbcopy")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    else {
        return false;
    };
    let written = child
        .stdin
        .take()
        .is_some_and(|mut stdin| std::io::Write::write_all(&mut stdin, text.as_bytes()).is_ok());
    child.wait().is_ok_and(|status| status.success()) && written
}
