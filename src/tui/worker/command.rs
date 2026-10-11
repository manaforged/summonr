use std::collections::VecDeque;

use tokio::sync::mpsc::UnboundedReceiver;

use super::reader::Reader;
use super::settings::{announce_run_mode, set_run_mode};
use super::{Cmd, Msg, Worker};

mod flow;
mod network;
mod session;
pub(super) mod shell;
mod work;
use crate::command::argument;
use crate::durable::log::{self, Line};
use crate::provider::UserInput;
use crate::tui::Activity;

struct Inbox<'a> {
    cmd: &'a mut UnboundedReceiver<Cmd>,
    reader: &'a Reader,
    followups: &'a mut VecDeque<UserInput>,
    deferred: &'a mut VecDeque<Cmd>,
    ui: &'a tokio::sync::mpsc::UnboundedSender<Msg>,
    answers: Option<tokio::sync::mpsc::UnboundedSender<String>>,
}

enum Attended<T> {
    Done(T),
    Cancelled,
    Quit,
}

async fn attend<T>(inbox: Inbox<'_>, task: impl Future<Output = T>) -> Attended<T> {
    tokio::pin!(task);
    loop {
        tokio::select! {
            result = &mut task => break Attended::Done(result),
            command = inbox.cmd.recv() => match command {
                Some(Cmd::Cancel) => break Attended::Cancelled,
                Some(Cmd::Quit) | None => break Attended::Quit,
                Some(Cmd::Tick) => {}
                Some(command @ (Cmd::ListModels { .. } | Cmd::ListPrompts(_) | Cmd::Clipboard)) => inbox.reader.run(command).await,
                Some(Cmd::LoginAnswer(code)) => {
                    if let Some(answers) = &inbox.answers {
                        drop(answers.send(code));
                    }
                }
                Some(Cmd::Submit(input) | Cmd::Steer(input) | Cmd::Queue(input)) => {
                    inbox.followups.push_back(input);
                    drop(inbox.ui.send(Msg::Queued {
                        steering: 0,
                        queued: inbox.followups.len(),
                    }));
                }
                Some(command) => inbox.deferred.push_back(command),
            }
        }
    }
}

impl Worker {}
