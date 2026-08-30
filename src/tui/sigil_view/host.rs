use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event as TermEvent,
};
use crossterm::style::Print;
use crossterm::terminal::{
    Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use crossterm::{cursor, execute, queue};
use sigil::prelude::Cx;
use sigil_hal::{Point, ShellEvent};
use sigil_tui::{EventRouter, Host, Screen};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use super::super::{Cmd, Info, Msg, worker};
use super::app::App;
use super::model::Entry;
use super::view::root;
use crate::error::{Error, Result};
use crate::provider::Client;
use crate::runtime::Loop;

const FRAME: Duration = Duration::from_millis(40);
const BUFFER: usize = 64 * 1024;

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
    let session = info.session.clone();
    let version = info.version.clone();
    let code = sigil::reactive::Runtime::scope("mage-tui", || {
        live(App::new(info, workspace, cmd_tx.clone()), ui_rx)
    })?;
    drop(cmd_tx.send(Cmd::Quit));
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => {
            tokio::task::block_in_place(|| {
                handle.block_on(async {
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
        }
        Err(_) => worker.abort(),
    }
    eprintln!(
        "\n  summon {version} · session {session}\n  resume: summon -c · by id: summon --session {session} · branch: summon --fork\n"
    );
    Ok(code)
}

pub(super) fn start_tick(tx: UnboundedSender<Cmd>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        loop {
            interval.tick().await;
            if tx.send(Cmd::Tick).is_err() {
                break;
            }
        }
    });
}

fn live(app: App, mut ui: UnboundedReceiver<Msg>) -> Result<i32> {
    guard();
    let mut term = Terminal::enter()?;
    let (mut cols, mut rows) = crossterm::terminal::size()?;
    app.resize(cols, rows);
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
    let mut status_at = Instant::now();
    loop {
        let mut incoming = Vec::new();
        if event::poll(FRAME)? {
            read_event(
                &mut incoming,
                &mut pointer,
                &mut cols,
                &mut rows,
                &mut screen,
                &mut term,
                &app,
            )?;
        }
        let mut changed = false;
        while let Ok(msg) = ui.try_recv() {
            app.update(msg);
            changed = true;
        }
        if let Some(code) = status(ui.is_closed(), app.quit.peek()) {
            return Ok(code);
        }
        if changed {
            app.refresh();
        } else if app.model.borrow().busy_since.is_some()
            && status_at.elapsed() >= Duration::from_secs(1)
        {
            app.refresh_status();
            status_at = Instant::now();
        }
        app.tick();
        frame(&mut screen, &mut host, &mut router, incoming, (cols, rows))?;
        if let Some(path) = app.open_file.peek() {
            app.open_file.set(None);
            term.suspend()?;
            let opened = open_external(&app.workspace.join(&path));
            term.resume()?;
            screen.invalidate();
            if let Err(error) = opened {
                app.model
                    .borrow_mut()
                    .entries
                    .push(Entry::Error(error.to_string()));
            }
            app.refresh();
        }
        if let Some(text) = app.copy.peek() {
            app.copy.set(None);
            term.copy(&text)?;
            app.copied(&text);
        }
        if app.editor_requested.peek() {
            app.editor_requested.set(false);
            term.suspend()?;
            let edited = edit_external(&app.draft.peek());
            term.resume()?;
            screen.invalidate();
            match edited {
                Ok(draft) => app.replace(draft),
                Err(error) => {
                    app.model
                        .borrow_mut()
                        .entries
                        .push(Entry::Error(error.to_string()));
                    app.refresh();
                }
            }
        }
        term.paint(&mut screen)?;
    }
}

fn status(closed: bool, quit: bool) -> Option<i32> {
    if quit {
        Some(0)
    } else if closed {
        Some(1)
    } else {
        None
    }
}

fn read_event(
    incoming: &mut Vec<ShellEvent>,
    pointer: &mut Point,
    cols: &mut u16,
    rows: &mut u16,
    screen: &mut Screen,
    term: &mut Terminal,
    app: &App,
) -> Result<()> {
    match event::read()? {
        TermEvent::Key(key) => incoming.extend(sigil_tui::map_key(&key)),
        TermEvent::Resize(next_cols, next_rows) => {
            *cols = next_cols;
            *rows = next_rows;
            app.resize(next_cols, next_rows);
            screen.invalidate();
            term.redraw = true;
        }
        TermEvent::Mouse(mouse) => {
            incoming.extend(sigil_tui::map_mouse(&mouse, Host::CELL, pointer));
        }
        TermEvent::Paste(text) => {
            if !app.paste(&text) {
                incoming.push(sigil_tui::paste(&text));
            }
        }
        TermEvent::FocusGained | TermEvent::FocusLost => {}
    }
    Ok(())
}

fn frame(
    screen: &mut Screen,
    host: &mut Host,
    router: &mut EventRouter,
    incoming: impl IntoIterator<Item = ShellEvent>,
    size: (u16, u16),
) -> Result<()> {
    screen
        .frame(host, router, incoming, size)
        .map(drop)
        .map_err(|error| Error::Tool(format!("Sigil layout failed: {error}")))
}

struct Terminal {
    out: BufWriter<std::io::Stdout>,
    redraw: bool,
}

impl Terminal {
    fn enter() -> Result<Self> {
        let mut terminal = Self {
            out: BufWriter::with_capacity(BUFFER, std::io::stdout()),
            redraw: true,
        };
        enable_raw_mode()?;
        execute!(
            terminal.out,
            EnterAlternateScreen,
            EnableMouseCapture,
            EnableBracketedPaste,
            cursor::Hide
        )?;
        Ok(terminal)
    }

    fn paint(&mut self, screen: &mut Screen) -> Result<()> {
        if !self.redraw && !screen.needs_flush() {
            return Ok(());
        }
        queue!(self.out, Print("\x1b[?2026h"))?;
        if self.redraw {
            queue!(self.out, Clear(ClearType::All))?;
            screen.invalidate();
            self.redraw = false;
        }
        screen.flush(&mut self.out)?;
        queue!(self.out, Print("\x1b[?2026l"))?;
        self.out.flush()?;
        Ok(())
    }

    /// Puts `text` on the clipboard through OSC 52; the terminal decides
    /// whether to honour it.
    fn copy(&mut self, text: &str) -> Result<()> {
        use base64::Engine as _;
        let encoded = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
        queue!(self.out, Print(format!("\x1b]52;c;{encoded}\x07")))?;
        self.out.flush()?;
        Ok(())
    }

    fn suspend(&mut self) -> Result<()> {
        restore(&mut self.out)?;
        disable_raw_mode()?;
        Ok(())
    }

    fn resume(&mut self) -> Result<()> {
        enable_raw_mode()?;
        execute!(
            self.out,
            EnterAlternateScreen,
            EnableMouseCapture,
            EnableBracketedPaste,
            cursor::Hide
        )?;
        self.redraw = true;
        Ok(())
    }
}

fn open_external(path: &std::path::Path) -> Result<()> {
    let editor = std::env::var("VISUAL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| std::env::var("EDITOR").ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "vi".to_string());
    let status = std::process::Command::new(&editor)
        .arg(path)
        .status()
        .map_err(|error| Error::Tool(format!("cannot start {editor}: {error}")))?;
    if !status.success() {
        return Err(Error::Tool(format!("editor exited with {status}")));
    }
    Ok(())
}

fn edit_external(initial: &str) -> Result<String> {
    let mut draft = tempfile::Builder::new().suffix(".md").tempfile()?;
    draft.write_all(initial.as_bytes())?;
    draft.flush()?;
    let editor = std::env::var("VISUAL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| std::env::var("EDITOR").ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "vi".to_string());
    let status = std::process::Command::new(&editor)
        .arg(draft.path())
        .status()
        .map_err(|error| Error::Tool(format!("cannot start {editor}: {error}")))?;
    if !status.success() {
        return Err(Error::Tool(format!("editor exited with {status}")));
    }
    let text = std::fs::read_to_string(draft.path())?;
    Ok(text.trim_end_matches('\n').to_string())
}

impl Drop for Terminal {
    fn drop(&mut self) {
        drop(restore(&mut self.out));
        drop(disable_raw_mode());
    }
}

fn restore(out: &mut impl Write) -> std::io::Result<()> {
    queue!(
        out,
        Print("\x1b[?2026l"),
        DisableBracketedPaste,
        DisableMouseCapture,
        cursor::Show,
        LeaveAlternateScreen
    )?;
    out.flush()
}

fn guard() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            drop(execute!(
                std::io::stdout(),
                Print("\x1b[?2026l"),
                DisableBracketedPaste,
                DisableMouseCapture,
                cursor::Show,
                LeaveAlternateScreen
            ));
            drop(disable_raw_mode());
            previous(info);
        }));
    });
}

#[cfg(test)]
#[path = "host_tests.rs"]
mod tests;
