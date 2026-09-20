use std::cell::RefCell;
use std::collections::VecDeque;
use std::fs::read_to_string;
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
use sigil::prelude::{
    ClipError, Clipboard, Cx, Key, LogicalKey, OsClipboard, Point, ShellEvent, Signal, install,
};
use sigil_tui::{EventRouter, Grid, Host, Screen, calibrate};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use super::app::{App, CopyStatus};
use super::model::Entry;
use super::view::root;
use crate::error::{Error, Result};
use crate::provider::Client;
use crate::runtime::Loop;
use crate::tui::{Cmd, Exit, Info, Msg, worker};

/// The animation clock. Scenes are authored in 40 ms frames, so they advance
/// on their own cadence whatever frame rate is configured.
mod selection;
use selection::Selection;

pub(in crate::tui::sigil_view) use intake::frame_budget;
#[cfg(feature = "gui")]
pub(in crate::tui::sigil_view) use run::start_tick;

mod edit;
pub(super) mod intake;
pub(super) mod run;
mod terminal;
