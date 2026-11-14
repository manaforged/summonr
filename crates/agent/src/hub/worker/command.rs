use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use super::reader::Reader;
use super::settings::{announce_run_mode, set_run_mode};
use super::steer::InputQueue;
use super::{Cmd, Msg, Worker};
use crate::hub::{Fact, Intent};

mod flow;
mod network;
mod session;
pub(super) mod shell;
mod work;
use crate::command::argument;
use crate::durable::log::{self, Line};
use crate::hub::Activity;
use crate::provider::UserInput;

enum Completion<T> {
    Done(T),
    Cancelled,
    Quit,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Waiting {
    Work,
    Login,
}

impl Worker {
    async fn wait_work<T>(
        &mut self,
        run: impl AsyncFnOnce(&mut crate::Loop<crate::provider::Client>) -> T,
    ) -> Completion<T> {
        wait_cancelable(
            &mut self.cmd,
            &mut self.queue,
            &self.reader,
            &self.ui,
            Waiting::Work,
            None,
            run(&mut self.loop_),
        )
        .await
    }
}

async fn wait_cancelable<T>(
    cmd: &mut UnboundedReceiver<Cmd>,
    queue: &mut InputQueue,
    reader: &Reader,
    ui: &UnboundedSender<Msg>,
    waiting: Waiting,
    answers: Option<UnboundedSender<String>>,
    task: impl Future<Output = T>,
) -> Completion<T> {
    tokio::pin!(task);
    loop {
        tokio::select! {
            result = &mut task => break Completion::Done(result),
            command = cmd.recv() => match command {
                Some(Cmd::Cancel) => break Completion::Cancelled,
                Some(Cmd::CancelLogin) => {
                    queue.drop_auth();
                    if waiting == Waiting::Login {
                        break Completion::Cancelled;
                    }
                }
                Some(Cmd::Quit) | None => break Completion::Quit,
                Some(Cmd::Tick | Cmd::Peers(_)) => {}
                Some(command @ (Cmd::ListModels { .. } | Cmd::ListPrompts(_) | Cmd::Clipboard)) => reader.run(command).await,
                Some(Cmd::LoginAnswer(code)) => {
                    if let Some(answers) = &answers {
                        drop(answers.send(code));
                    }
                }
                Some(Cmd::Submit(input) | Cmd::Steer(input) | Cmd::Queue(input)) => {
                    queue.followups.push_back(input);
                    queue.announce(ui);
                }
                Some(command) => queue.deferred.push_back(command),
            }
        }
    }
}

#[cfg(test)]
#[path = "command_tests.rs"]
mod tests;
