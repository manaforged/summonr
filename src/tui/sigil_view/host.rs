use std::cell::RefCell;
use std::fs::read_to_string;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use sigil::events::ShellEvent;
use sigil::prelude::{Key, LogicalKey, ModifiersState};
use sigil::terminal::{Hooks, bell, calibrate, focused, set_mouse_capture, set_title, suspend};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use super::app::App;
use super::model::Entry;
use super::view::root;
use crate::error::{Error, Result};
use crate::provider::Client;
use crate::runtime::Loop;
use crate::tui::{Cmd, Exit, Info, Msg, worker};

#[cfg(feature = "gui")]
pub(in crate::tui::sigil_view) use run::{copy_out, start_tick, stop};

mod edit;
mod intake;
pub(super) mod run;
