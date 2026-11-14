use std::cell::RefCell;
use std::fs::read_to_string;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use sigil::events::Event;
use sigil::prelude::{Key, LogicalKey, ModifiersState};
use sigil::terminal::{
    Hooks, MouseTracking, bell, calibrate, focused, set_mouse_tracking, set_title, suspend,
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use super::app::App;
use super::model::Entry;
use super::view::root;
use summonr_agent::hub::{Cmd, Info, Msg};
use summonr_agent::hub::{Factory, Started, start};
use summonr_agent::provider::Client;
use summonr_agent::runtime::Loop;
use summonr_core::error::{Error, Result};

mod edit;
pub(super) mod intake;
pub(crate) mod run;
