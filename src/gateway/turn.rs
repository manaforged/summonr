use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::oneshot;
use tokio::task::{Id, JoinError, JoinSet};

use super::child::turn;
use super::store;
use super::telegram::{Bot, ChatKind, Poll, Update, User};
use super::{Action, addressed, route};
use crate::config::tilde;
use crate::limits::Reach;
use summonr_core::error::Result;

const IDLE: &str = "nothing is running";
const STOPPING: &str = "stopping";
const CRASHED: &str = "error: the turn crashed";
const FRESH: &str = "new session";

#[derive(Clone, Debug, Default)]
pub(crate) struct Options {
    pub endpoint: Option<String>,
    pub model: Option<String>,
    pub dir: PathBuf,
}

struct Running {
    stop: Option<oneshot::Sender<()>>,
    queue: VecDeque<(i64, String)>,
}

type Finished = (i64, u64, Option<String>);

struct Bridge {
    bot: Arc<Bot>,
    options: Arc<Options>,
    username: Option<String>,
    chats: HashMap<i64, Running>,
    generations: HashMap<i64, u64>,
    tasks: JoinSet<Finished>,
    owners: HashMap<Id, i64>,
}

pub(crate) async fn serve(bot: Bot, options: Options, username: Option<String>) -> Result<i32> {
    let mut bridge = Bridge {
        bot: Arc::new(bot),
        options: Arc::new(options),
        username,
        chats: HashMap::new(),
        generations: HashMap::new(),
        tasks: JoinSet::new(),
        owners: HashMap::new(),
    };
    let mut interrupt = std::pin::pin!(crate::interrupted());
    let mut offset = store::load()?.offset;
    let mut failures = 0_u32;
    let mut retry_after = None;
    loop {
        tokio::select! {
            code = &mut interrupt => {
                bridge.tasks.abort_all();
                return Ok(code);
            }
            Some(done) = bridge.tasks.join_next_with_id(), if !bridge.tasks.is_empty() => {
                bridge.joined(done).await?;
            }
            polled = poll(&bridge.bot, offset, failures, retry_after) => match polled? {
                Poll::Updates(updates) => {
                    failures = 0;
                    bridge.drain(updates, &mut offset).await?;
                }
                Poll::Retry { error, after } => {
                    eprintln!("{error}");
                    failures = failures.saturating_add(1);
                    retry_after = after;
                }
            }
        }
    }
}

async fn poll(bot: &Bot, offset: i64, failures: u32, after: Option<u64>) -> Result<Poll> {
    if let Some(retries) = failures.checked_sub(1) {
        let limits = crate::limits::limits();
        let backoff = limits.retry_policy(Reach::Remote).delay(retries);
        tokio::time::sleep(limits.retry_wait(after, retries).max(backoff)).await;
    }
    bot.updates(offset).await
}

impl Bridge {
    async fn drain(&mut self, updates: Vec<Update>, offset: &mut i64) -> Result<()> {
        for update in updates {
            let next = update.update_id.saturating_add(1);
            self.handle(update).await?;
            *offset = (*offset).max(next);
            let saved = *offset;
            store::update(|state| state.offset = saved)?;
        }
        Ok(())
    }

    async fn joined(&mut self, done: std::result::Result<(Id, Finished), JoinError>) -> Result<()> {
        match done {
            Ok((id, finished)) => {
                self.owners.remove(&id);
                self.finish(finished)
            }
            Err(error) => {
                let Some(chat) = self.owners.remove(&error.id()) else {
                    return Ok(());
                };
                eprintln!("the turn for Telegram chat {chat} failed: {error}");
                say(&self.bot, chat, CRASHED).await;
                self.finish((chat, self.generation(chat), None))
            }
        }
    }

    fn accept(&self, update: Update) -> Option<(User, i64, String)> {
        let message = update.message?;
        if message.chat.kind != ChatKind::Private {
            return None;
        }
        let (user, text) = (message.from?, message.text?);
        addressed(&text, self.username.as_deref()).then_some((user, message.chat.id, text))
    }

    async fn handle(&mut self, update: Update) -> Result<()> {
        let Some((user, chat, text)) = self.accept(update) else {
            return Ok(());
        };
        if let Some(reply) = self.reply(&user, chat, &text)? {
            say(&self.bot, chat, &reply).await;
        }
        Ok(())
    }

    fn reply(&mut self, user: &User, chat: i64, text: &str) -> Result<Option<String>> {
        let now = crate::cron::now();
        let dir = tilde(&self.options.dir);
        Ok(match route(&store::load()?, user.id, text, now) {
            Action::Drop => None,
            Action::Pair => Some(pair(user, chat, now)?),
            Action::New => {
                let generation = self.generations.entry(chat).or_default();
                *generation = generation.wrapping_add(1);
                store::update(|state| state.set_session(chat, None))?;
                Some(FRESH.to_string())
            }
            Action::Status => Some(self.status(chat, &dir)?),
            Action::Help => Some(format!(
                "Send a message and I work on it in {dir}. /new starts fresh, /status shows the session, /stop ends the running turn."
            )),
            Action::Stop => self.stop(chat),
            Action::Run(text) => self.run(chat, user.id, text),
        })
    }

    fn status(&self, chat: i64, dir: &str) -> Result<String> {
        let state = store::load()?;
        let session = state.session(chat).unwrap_or("none");
        let (running, queued) = self
            .chats
            .get(&chat)
            .map_or(("no", 0), |running| ("yes", running.queue.len()));
        Ok(format!(
            "session: {session}\nrunning: {running}\nqueued: {queued}\ndir: {dir}"
        ))
    }

    fn stop(&mut self, chat: i64) -> Option<String> {
        let Some(running) = self.chats.get_mut(&chat) else {
            return Some(IDLE.to_string());
        };
        running.queue.clear();
        let sent = running
            .stop
            .take()
            .is_some_and(|stop| stop.send(()).is_ok());
        (!sent).then(|| STOPPING.to_string())
    }

    fn run(&mut self, chat: i64, user: i64, text: String) -> Option<String> {
        if let Some(running) = self.chats.get_mut(&chat) {
            running.queue.push_back((user, text));
            return Some(format!(
                "queued ({} ahead); I'll start it when the current turn ends.",
                running.queue.len()
            ));
        }
        let stop = self.start(chat, text);
        self.chats.insert(
            chat,
            Running {
                stop: Some(stop),
                queue: VecDeque::new(),
            },
        );
        None
    }

    fn generation(&self, chat: i64) -> u64 {
        self.generations.get(&chat).copied().unwrap_or_default()
    }

    fn start(&mut self, chat: i64, text: String) -> oneshot::Sender<()> {
        let (stop, stopped) = oneshot::channel();
        let bot = Arc::clone(&self.bot);
        let options = Arc::clone(&self.options);
        let generation = self.generation(chat);
        let task = self.tasks.spawn(async move {
            let (session, reply) = turn(&bot, &options, chat, &text, stopped)
                .await
                .unwrap_or_else(|error| (None, format!("error: {error}")));
            say(&bot, chat, &reply).await;
            (chat, generation, session)
        });
        self.owners.insert(task.id(), chat);
        stop
    }

    fn finish(&mut self, (chat, generation, session): Finished) -> Result<()> {
        if let Some(id) = session.filter(|_| generation == self.generation(chat)) {
            store::update(|state| state.set_session(chat, Some(id)))?;
        }
        let state = store::load()?;
        let queued = self.chats.get_mut(&chat).and_then(|running| {
            running.queue.retain(|(user, _)| state.paired(*user));
            running.queue.pop_front()
        });
        let Some((_, text)) = queued else {
            self.chats.remove(&chat);
            return Ok(());
        };
        let stop = self.start(chat, text);
        if let Some(running) = self.chats.get_mut(&chat) {
            running.stop = Some(stop);
        }
        Ok(())
    }
}

fn pair(user: &User, chat: i64, now: i64) -> Result<String> {
    let code = store::new_code();
    store::update(|state| state.offer(user.id, chat, &code, now))?;
    eprintln!(
        "pairing request from {}; they were sent a code",
        user.describe()
    );
    Ok(format!(
        "To pair, run on your machine:\nsummonr gateway pair {code}"
    ))
}

async fn say(bot: &Bot, chat: i64, text: &str) {
    if let Err(error) = bot.send(chat, text).await {
        eprintln!("{error}");
    }
}
