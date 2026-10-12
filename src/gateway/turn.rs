use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::oneshot;
use tokio::task::JoinSet;

use super::store;
use super::telegram::{Bot, ChatKind, Update, User};
use super::{Action, addressed, route};
use crate::error::Result;

use super::child::turn;

const INTERRUPTED: i32 = 130;
const IDLE: &str = "nothing is running";
const FRESH: &str = "new session";

#[derive(Clone, Debug, Default)]
pub(crate) struct Options {
    pub endpoint: Option<String>,
    pub model: Option<String>,
    pub dir: PathBuf,
}

struct Queued {
    user: i64,
    text: String,
}

struct Running {
    stop: Option<oneshot::Sender<()>>,
    queue: VecDeque<Queued>,
}

struct Finished {
    chat: i64,
    generation: u64,
    session: Option<String>,
}

enum Wake {
    Quit,
    Done(Option<Finished>),
    Updates(Result<Vec<Update>>),
}

struct Bridge {
    bot: Arc<Bot>,
    options: Arc<Options>,
    username: Option<String>,
    chats: HashMap<i64, Running>,
    generations: HashMap<i64, u64>,
    tasks: JoinSet<Finished>,
}

pub(crate) async fn serve(bot: Bot, options: Options, username: Option<String>) -> Result<i32> {
    let mut bridge = Bridge {
        bot: Arc::new(bot),
        options: Arc::new(options),
        username,
        chats: HashMap::new(),
        generations: HashMap::new(),
        tasks: JoinSet::new(),
    };
    let mut interrupt = std::pin::pin!(tokio::signal::ctrl_c());
    let mut offset = store::load()?.offset;
    loop {
        let wake = tokio::select! {
            _ = &mut interrupt => Wake::Quit,
            Some(done) = bridge.tasks.join_next(), if !bridge.tasks.is_empty() => Wake::Done(done.ok()),
            updates = bridge.bot.updates(offset) => Wake::Updates(updates),
        };
        match wake {
            Wake::Quit => {
                bridge.tasks.abort_all();
                return Ok(INTERRUPTED);
            }
            Wake::Done(Some(finished)) => bridge.finish(finished)?,
            Wake::Done(None) => {}
            Wake::Updates(updates) => {
                for update in updates? {
                    offset = offset.max(update.update_id.saturating_add(1));
                    store::update(|state| {
                        state.offset = offset;
                        Ok(())
                    })?;
                    bridge.handle(update).await?;
                }
            }
        }
    }
}

impl Bridge {
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
        let now = crate::cron::now();
        let action = route(&store::load()?, user.id, chat, &text, now);
        match action {
            Action::Drop => {}
            Action::Pair => self.pair(&user, chat, now).await?,
            Action::New => self.fresh(chat).await?,
            Action::Status => {
                let text = self.status(chat)?;
                self.say(chat, &text).await;
            }
            Action::Help => {
                let text = format!(
                    "Send a message and I work on it in {}. /new starts fresh, /status shows the session, /stop ends the running turn.",
                    crate::config::tilde(&self.options.dir)
                );
                self.say(chat, &text).await;
            }
            Action::Stop => self.stop(chat).await,
            Action::Run(text) => self.run(chat, user.id, text).await,
        }
        Ok(())
    }

    async fn fresh(&mut self, chat: i64) -> Result<()> {
        let generation = self.generations.entry(chat).or_default();
        *generation = generation.wrapping_add(1);
        store::update(|state| {
            state.set_session(chat, None);
            Ok(())
        })?;
        self.say(chat, FRESH).await;
        Ok(())
    }

    async fn pair(&self, user: &User, chat: i64, now: i64) -> Result<()> {
        let code = store::new_code();
        store::update(|state| {
            state.offer(user.id, chat, &code, now);
            Ok(())
        })?;
        eprintln!(
            "pairing request from {}; they were sent a code",
            user.describe()
        );
        self.say(
            chat,
            &format!("To pair, run on your machine:\nsummonr gateway pair {code}"),
        )
        .await;
        Ok(())
    }

    async fn stop(&mut self, chat: i64) {
        let stopped = self.chats.get_mut(&chat).is_some_and(|running| {
            running.queue.clear();
            running
                .stop
                .take()
                .is_some_and(|stop| stop.send(()).is_ok())
        });
        if !stopped {
            self.say(chat, IDLE).await;
        }
    }

    async fn run(&mut self, chat: i64, user: i64, text: String) {
        if let Some(running) = self.chats.get_mut(&chat) {
            running.queue.push_back(Queued { user, text });
            let ahead = running.queue.len();
            self.say(
                chat,
                &format!("queued ({ahead} ahead); I'll start it when the current turn ends."),
            )
            .await;
            return;
        }
        let stop = self.start(chat, text);
        self.chats.insert(
            chat,
            Running {
                stop: Some(stop),
                queue: VecDeque::new(),
            },
        );
    }

    fn status(&self, chat: i64) -> Result<String> {
        let state = store::load()?;
        let session = state.session(chat).unwrap_or("none");
        let (running, queued) = self
            .chats
            .get(&chat)
            .map_or((false, 0), |running| (true, running.queue.len()));
        Ok(format!(
            "session: {session}\nrunning: {}\nqueued: {queued}\ndir: {}",
            if running { "yes" } else { "no" },
            crate::config::tilde(&self.options.dir)
        ))
    }

    async fn say(&self, chat: i64, text: &str) {
        if let Err(error) = self.bot.send(chat, text).await {
            eprintln!("{error}");
        }
    }

    fn generation(&self, chat: i64) -> u64 {
        self.generations.get(&chat).copied().unwrap_or_default()
    }

    fn start(&mut self, chat: i64, text: String) -> oneshot::Sender<()> {
        let (stop, stopped) = oneshot::channel();
        let bot = Arc::clone(&self.bot);
        let options = Arc::clone(&self.options);
        let generation = self.generation(chat);
        self.tasks.spawn(async move {
            let (session, reply) = match turn(&bot, &options, chat, &text, stopped).await {
                Ok(done) => (done.session, done.reply),
                Err(error) => (None, format!("error: {error}")),
            };
            if let Err(error) = bot.send(chat, &reply).await {
                eprintln!("{error}");
            }
            Finished {
                chat,
                generation,
                session,
            }
        });
        stop
    }

    fn finish(&mut self, finished: Finished) -> Result<()> {
        let Finished {
            chat,
            generation,
            session,
        } = finished;
        if let Some(id) = session.filter(|_| generation == self.generation(chat)) {
            store::update(|state| {
                state.set_session(chat, Some(id));
                Ok(())
            })?;
        }
        self.next(chat);
        Ok(())
    }

    fn next(&mut self, chat: i64) {
        let state = store::load().ok();
        let queued = self.chats.get_mut(&chat).and_then(|running| {
            running.queue.retain(|queued| {
                state
                    .as_ref()
                    .is_some_and(|state| state.paired(queued.user))
            });
            running.queue.pop_front()
        });
        let Some(queued) = queued else {
            self.chats.remove(&chat);
            return;
        };
        let stop = self.start(chat, queued.text);
        if let Some(running) = self.chats.get_mut(&chat) {
            running.stop = Some(stop);
        }
    }
}
