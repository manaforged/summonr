use super::run::pbcopy;
use super::*;

pub(in crate::tui::sigil_view) const BUFFER: usize = 64 * 1024;

pub(in crate::tui::sigil_view) struct Paste {
    pub(in crate::tui::sigil_view) text: Rc<RefCell<Option<String>>>,
    pub(in crate::tui::sigil_view) copy: Signal<Option<String>>,
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

pub(in crate::tui::sigil_view) struct Terminal {
    pub(in crate::tui::sigil_view) paste: Rc<RefCell<Option<String>>>,
    pub(in crate::tui::sigil_view) out: BufWriter<std::io::Stdout>,
    pub(in crate::tui::sigil_view) focused: bool,
    pub(in crate::tui::sigil_view) redraw: bool,
    pub(in crate::tui::sigil_view) anchor: Option<(u16, u16)>,
    pub(in crate::tui::sigil_view) selection: Option<Selection>,
}

impl Terminal {
    pub(in crate::tui::sigil_view) fn enter() -> Result<Self> {
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

    pub(in crate::tui::sigil_view) fn paint(&mut self, screen: &mut Screen) -> Result<()> {
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

    pub(in crate::tui::sigil_view) fn copy(&mut self, text: &str) -> Result<CopyStatus> {
        if pbcopy(text) {
            return Ok(CopyStatus::Confirmed);
        }
        osc52(&mut self.out, text)
    }

    pub(in crate::tui::sigil_view) fn select(&mut self, on: bool) -> Result<()> {
        mouse(&mut self.out, on)?;
        self.redraw = true;
        Ok(())
    }

    pub(in crate::tui::sigil_view) fn deselect(&mut self, screen: &mut Screen) {
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

    pub(in crate::tui::sigil_view) fn suspend(&mut self) -> Result<()> {
        restore(&mut self.out)?;
        disable_raw_mode()?;
        Ok(())
    }

    pub(in crate::tui::sigil_view) fn resume(&mut self, selecting: bool) -> Result<()> {
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

    pub(in crate::tui::sigil_view) fn title(&mut self, text: &str) -> Result<()> {
        queue!(self.out, SetTitle(text))?;
        Ok(())
    }

    pub(in crate::tui::sigil_view) fn bell(&mut self) -> Result<()> {
        queue!(self.out, Print("\x1b]9;summonr: turn finished\x07\x07"))?;
        Ok(())
    }
}

pub(in crate::tui::sigil_view) fn mouse(
    out: &mut impl Write,
    selecting: bool,
) -> std::io::Result<()> {
    if selecting {
        queue!(out, DisableMouseCapture)?;
    } else {
        queue!(out, EnableMouseCapture)?;
    }
    out.flush()
}

pub(in crate::tui::sigil_view) fn osc52(out: &mut impl Write, text: &str) -> Result<CopyStatus> {
    let encoded = STANDARD.encode(text.as_bytes());
    queue!(out, Print(format!("\x1b]52;c;{encoded}\x07")))?;
    out.flush()?;
    Ok(CopyStatus::Requested)
}

impl Drop for Terminal {
    fn drop(&mut self) {
        drop(restore(&mut self.out));
        drop(disable_raw_mode());
    }
}

pub(in crate::tui::sigil_view) fn restore(out: &mut impl Write) -> std::io::Result<()> {
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
