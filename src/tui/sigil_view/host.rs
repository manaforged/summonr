use std::cell::RefCell;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use crossterm::event::{
    self, DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
    EnableFocusChange, EnableMouseCapture, Event as TermEvent, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, MouseButton, MouseEventKind,
};
use crossterm::style::Print;
use crossterm::terminal::{
    Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, SetTitle, disable_raw_mode,
    enable_raw_mode,
};
use crossterm::{cursor, execute, queue};
use sigil::clipboard::OsClipboard;
use sigil::prelude::{Cx, Signal};
use sigil_hal::{Point, ShellEvent};
use sigil_tui::widgets::{ClipError, Clipboard, install};
use sigil_tui::{EventRouter, Grid, Host, Screen, calibrate};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use super::super::{Cmd, Exit, Info, Msg, worker};
use super::app::{App, CopyStatus};
use super::model::Entry;
use super::view::root;
use crate::error::{Error, Result};
use crate::provider::Client;
use crate::runtime::Loop;

const FRAME: Duration = Duration::from_millis(40);
const BUFFER: usize = 64 * 1024;

mod selection;
use selection::Selection;

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
    let (exit, session) = sigil::reactive::Runtime::scope("mage-tui", || {
        let app = App::new(info, workspace, cmd_tx.clone());
        let exit = live(app.clone(), ui_rx);
        let session = app.model.borrow().session.clone();
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
    if matches!(exit, Exit::Code(_)) {
        eprintln!(
            "\n  summon {version} · session {session}\n  resume: summon -c · by id: summon --session {session} · branch: summon --fork\n"
        );
    }
    Ok(exit)
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

fn live(app: App, mut ui: UnboundedReceiver<Msg>) -> Result<Exit> {
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
    let mut was_busy = None;
    term.title(&title(&app, false))?;
    loop {
        let mut incoming = Vec::new();
        if event::poll(FRAME)? {
            read_event(
                event::read()?,
                &mut incoming,
                &mut pointer,
                &mut screen,
                &mut term,
                &app,
            )?;
            (cols, rows) = app.viewport.peek();
        }
        let mut changed = false;
        while let Ok(msg) = ui.try_recv() {
            app.update(msg);
            changed = true;
        }
        if let Some(id) = app.resume.borrow().clone() {
            return Ok(Exit::Resume(id));
        }
        if let Some(code) = status(ui.is_closed(), app.quit.peek()) {
            return Ok(Exit::Code(code));
        }
        let busy = app.model.borrow().busy_since.is_some();
        if was_busy != Some(busy) {
            term.title(&title(&app, busy))?;
            if was_busy == Some(true) && !busy && !term.focused {
                term.bell()?;
            }
            was_busy = Some(busy);
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
        let grid = frame(&mut screen, &mut host, &mut router, incoming, (cols, rows))?;
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
            let opened = open_external(&path);
            term.resume(selecting)?;
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
            app.copied(&text, term.copy(&text)?);
        }
        if app.editor_requested.peek() {
            app.editor_requested.set(false);
            term.suspend()?;
            let edited = edit_external(&app.draft.peek());
            term.resume(selecting)?;
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

fn title(app: &App, busy: bool) -> String {
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
    event: TermEvent,
    incoming: &mut Vec<ShellEvent>,
    pointer: &mut Point,
    screen: &mut Screen,
    term: &mut Terminal,
    app: &App,
) -> Result<()> {
    match event {
        TermEvent::Key(key) => {
            if key.code == KeyCode::Char('c')
                && key.modifiers.contains(KeyModifiers::CONTROL)
                && let Some(text) = term.selection.as_ref().and_then(Selection::text)
            {
                app.copied(&text, term.copy(&text)?);
            } else if key.code == KeyCode::Char('v')
                && key.kind != KeyEventKind::Release
                && key.modifiers == KeyModifiers::CONTROL
                && app.clipboard()
            {
                term.deselect(screen);
            } else {
                term.deselect(screen);
                incoming.extend(sigil_tui::map_key(&key));
            }
        }
        TermEvent::Resize(next_cols, next_rows) => {
            term.deselect(screen);
            app.resize(next_cols, next_rows);
            screen.invalidate();
            term.redraw = true;
        }
        TermEvent::Mouse(mouse) => {
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left)
                    if !app.options_active()
                        && !app.splash.peek()
                        && mouse.column < app.chat_cols()
                        && mouse.row
                            < super::chat::prompt_row(app.viewport.peek().1, &app.draft.peek()) =>
                {
                    term.deselect(screen);
                    term.anchor = Some((mouse.column, mouse.row));
                    return Ok(());
                }
                MouseEventKind::Drag(MouseButton::Left) => {
                    if let Some(selection) = &mut term.selection {
                        selection.move_to(mouse.column, mouse.row);
                        term.redraw = true;
                        return Ok(());
                    }
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    if let Some(selection) = &mut term.selection {
                        selection.move_to(mouse.column, mouse.row);
                        if let Some(text) = selection.text() {
                            app.copied(&text, term.copy(&text)?);
                            term.redraw = true;
                        }
                        return Ok(());
                    }
                }
                MouseEventKind::Moved => return Ok(()),
                MouseEventKind::ScrollUp
                | MouseEventKind::ScrollDown
                | MouseEventKind::ScrollLeft
                | MouseEventKind::ScrollRight
                | MouseEventKind::Down(_) => term.deselect(screen),
                _ => {}
            }
            incoming.extend(sigil_tui::map_mouse(&mouse, Host::CELL, pointer));
        }
        TermEvent::Paste(text) => {
            let text = text.replace("\r\n", "\n").replace('\r', "\n");
            term.deselect(screen);
            if app.pane_active().is_some() && !app.options_active() {
                app.focus_pane(false);
            }
            if app.paste(&text) || app.paste_text(&text) {
            } else {
                *term.paste.borrow_mut() = Some(text);
                incoming.extend(sigil_tui::map_key(&KeyEvent::new(
                    KeyCode::Char('v'),
                    KeyModifiers::CONTROL,
                )));
            }
        }
        TermEvent::FocusGained => term.focused = true,
        TermEvent::FocusLost => term.focused = false,
    }
    Ok(())
}

fn frame<'a>(
    screen: &'a mut Screen,
    host: &mut Host,
    router: &mut EventRouter,
    incoming: impl IntoIterator<Item = ShellEvent>,
    size: (u16, u16),
) -> Result<&'a Grid> {
    screen
        .frame(host, router, incoming, size)
        .map_err(|error| Error::Tool(format!("Sigil layout failed: {error}")))
}

struct Paste {
    text: Rc<RefCell<Option<String>>>,
    copy: Signal<Option<String>>,
}

impl Clipboard for Paste {
    fn get(&self) -> Option<String> {
        self.text.borrow_mut().take().or_else(|| OsClipboard.get())
    }

    fn set(&self, text: &str) -> std::result::Result<(), ClipError> {
        self.copy.set(Some(text.to_string()));
        Ok(())
    }
}

struct Terminal {
    paste: Rc<RefCell<Option<String>>>,
    out: BufWriter<std::io::Stdout>,
    focused: bool,
    redraw: bool,
    anchor: Option<(u16, u16)>,
    selection: Option<Selection>,
}

impl Terminal {
    fn enter() -> Result<Self> {
        let mut terminal = Self {
            paste: Rc::default(),
            out: BufWriter::with_capacity(BUFFER, std::io::stdout()),
            focused: true,
            redraw: true,
            anchor: None,
            selection: None,
        };
        enable_raw_mode()?;
        execute!(terminal.out, EnterAlternateScreen)?;
        mouse(&mut terminal.out, false)?;
        execute!(
            terminal.out,
            EnableBracketedPaste,
            EnableFocusChange,
            cursor::Hide
        )?;
        Ok(terminal)
    }

    fn paint(&mut self, screen: &mut Screen) -> Result<()> {
        if let Some(selection) = &self.selection
            && selection.active()
        {
            if self.redraw {
                queue!(self.out, Print("\x1b[?2026h"))?;
                selection.paint(&mut self.out)?;
                queue!(self.out, Print("\x1b[?2026l"))?;
                self.out.flush()?;
                self.redraw = false;
            }
            return Ok(());
        }
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

    fn copy(&mut self, text: &str) -> Result<CopyStatus> {
        if pbcopy(text) {
            return Ok(CopyStatus::Confirmed);
        }
        osc52(&mut self.out, text)
    }

    fn select(&mut self, on: bool) -> Result<()> {
        mouse(&mut self.out, on)?;
        self.redraw = true;
        Ok(())
    }

    fn deselect(&mut self, screen: &mut Screen) {
        self.anchor = None;
        if self
            .selection
            .take()
            .is_some_and(|selection| selection.active())
        {
            self.redraw = true;
            screen.invalidate();
        }
    }

    fn suspend(&mut self) -> Result<()> {
        restore(&mut self.out)?;
        disable_raw_mode()?;
        Ok(())
    }

    fn resume(&mut self, selecting: bool) -> Result<()> {
        enable_raw_mode()?;
        execute!(self.out, EnterAlternateScreen)?;
        mouse(&mut self.out, selecting)?;
        execute!(
            self.out,
            EnableBracketedPaste,
            EnableFocusChange,
            cursor::Hide
        )?;
        self.redraw = true;
        Ok(())
    }

    fn title(&mut self, text: &str) -> Result<()> {
        queue!(self.out, SetTitle(text))?;
        Ok(())
    }

    fn bell(&mut self) -> Result<()> {
        queue!(self.out, Print("\x07"))?;
        Ok(())
    }
}

const DRAG: &str = "\x1b[?1003l\x1b[?1002h";

fn mouse(out: &mut impl Write, selecting: bool) -> std::io::Result<()> {
    if selecting {
        queue!(out, DisableMouseCapture)?;
    } else {
        queue!(out, EnableMouseCapture, Print(DRAG))?;
    }
    out.flush()
}

fn osc52(out: &mut impl Write, text: &str) -> Result<CopyStatus> {
    let encoded = STANDARD.encode(text.as_bytes());
    queue!(out, Print(format!("\x1b]52;c;{encoded}\x07")))?;
    out.flush()?;
    Ok(CopyStatus::Requested)
}

fn open_external(path: &Path) -> Result<()> {
    let editor = std::env::var("VISUAL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| std::env::var("EDITOR").ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "vi".to_string());
    edit(path, &editor)
}

fn edit(path: &Path, editor: &str) -> Result<()> {
    #[cfg(windows)]
    let mut command = {
        let mut command = Command::new("cmd.exe");
        command
            .args(["/D", "/V:OFF", "/S", "/C"])
            .env("SUMMONER_EDITOR_FILE", path)
            .raw_arg(format!("\"{editor} \"%SUMMONER_EDITOR_FILE%\"\""));
        command
    };
    #[cfg(not(windows))]
    let mut command = {
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg(format!("exec {editor} \"$1\""))
            .arg("summoner-editor")
            .arg(path);
        command
    };
    let status = command
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
    open_external(draft.path())?;
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
        DisableFocusChange,
        DisableMouseCapture,
        SetTitle(""),
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

#[cfg(test)]
#[path = "host_tests.rs"]
mod tests;

fn pbcopy(text: &str) -> bool {
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
