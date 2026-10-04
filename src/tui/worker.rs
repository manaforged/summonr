use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use super::{Cmd, Info, Msg};

use crate::provider::Client;
use crate::provider::UserInput;
use crate::runtime::Loop;
use crate::workflow::Workflow;

mod command;
mod dispatch;
mod export;

pub(super) use export::{announce, export_session};
mod history;
mod reader;
mod settings;
mod state;
mod turn;

use reader::Reader;
use settings::set_run_mode;

pub(super) async fn worker(
    loop_: Loop<Client>,
    cmd: UnboundedReceiver<Cmd>,
    ui: UnboundedSender<Msg>,
    info: Info,
) {
    Worker::new(loop_, cmd, ui, info).run().await;
}

struct Worker {
    loop_: Loop<Client>,
    cmd: UnboundedReceiver<Cmd>,
    ui: UnboundedSender<Msg>,
    endpoint: String,
    relay: Option<String>,
    auto_reply: bool,
    pending_mail: bool,
    last_auto: Instant,
    steering: Arc<crate::runtime::Steering>,
    deferred: VecDeque<Cmd>,
    followups: VecDeque<UserInput>,
    carried: usize,
    last_turn: Option<crate::Turn>,
    mail_error: Option<String>,
    workflow: Workflow,
    dial: Option<crate::Effort>,
    home: Option<std::path::PathBuf>,
    last_diff: Option<crate::diff::Diff>,
    last_peers: Option<Vec<crate::mail::Agent>>,
    reader: Reader,
}

impl Worker {}
