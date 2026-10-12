use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::oneshot;
use tokio::task::JoinSet;

use super::store;
use super::telegram::{Bot, Update, User};
use super::{Action, route};
use crate::Event;
use crate::error::{Error, Result};

const TYPING_EVERY: Duration = Duration::from_secs(4);
const INTERRUPTED: i32 = 130;
const NO_REPLY: &str = "(no reply)";
const STOPPED: &str = "stopped";
const IDLE: &str = "nothing is running";
const FRESH: &str = "new session";

#[derive(Clone, Debug, Default)]
pub(crate) struct Options {
    pub endpoint: Option<String>,
    pub model: Option<String>,
    pub dir: PathBuf,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Seen {
    pub session: Option<String>,
    pub text: Option<String>,
    pub aborted: Option<String>,
}

impl Seen {
    pub(crate) fn read(&mut self, line: &str) {
        match serde_json::from_str::<Event>(line) {
            Ok(Event::Session { id }) => self.session = Some(id),
            Ok(Event::Assistant { text }) if !text.trim().is_empty() => self.text = Some(text),
            Ok(Event::Aborted { reason, .. }) => self.aborted = Some(reason),
            Ok(_) | Err(_) => {}
        }
    }

    pub(crate) fn reply(&self) -> String {
        let text = self.text.as_deref().unwrap_or(NO_REPLY);
        match &self.aborted {
            Some(reason) => format!("{text}\n\naborted: {reason}"),
            None => text.to_string(),
        }
    }
}

pub(crate) fn argv(options: &Options, session: Option<&str>, text: &str) -> Vec<String> {
    let mut args: Vec<String> = options.endpoint.iter().cloned().collect();
    if let Some(model) = &options.model {
        args.extend(["--model".to_string(), model.clone()]);
    }
    if let Some(session) = session {
        args.extend(["--session".to_string(), session.to_string()]);
    }
    args.extend([
        "--json".to_string(),
        "-p".to_string(),
        "--".to_string(),
        text.to_string(),
    ]);
    args
}

struct Running {
    stop: Option<oneshot::Sender<()>>,
    queue: VecDeque<String>,
}

enum Wake {
    Quit,
    Done(Option<i64>),
    Updates(Result<Vec<Update>>),
}

struct Bridge {
    bot: Arc<Bot>,
    options: Arc<Options>,
    chats: HashMap<i64, Running>,
    tasks: JoinSet<i64>,
}

pub(crate) async fn serve(bot: Bot, options: Options) -> Result<i32> {
    let mut bridge = Bridge {
        bot: Arc::new(bot),
        options: Arc::new(options),
        chats: HashMap::new(),
        tasks: JoinSet::new(),
    };
    let mut interrupt = std::pin::pin!(tokio::signal::ctrl_c());
    let mut offset = 0;
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
            Wake::Done(Some(chat)) => bridge.next(chat),
            Wake::Done(None) => {}
            Wake::Updates(updates) => {
                for update in updates? {
                    offset = offset.max(update.update_id.saturating_add(1));
                    bridge.handle(update).await?;
                }
            }
        }
    }
}

impl Bridge {
    async fn handle(&mut self, update: Update) -> Result<()> {
        let Some(message) = update.message else {
            return Ok(());
        };
        let (Some(user), Some(text)) = (message.from, message.text) else {
            return Ok(());
        };
        let chat = message.chat.id;
        let now = crate::cron::now();
        let action = route(&store::load()?, user.id, chat, &text, now);
        match action {
            Action::Drop => {}
            Action::Pair => self.pair(&user, chat, now).await?,
            Action::New => {
                store::update(|state| {
                    state.set_session(chat, None);
                    Ok(())
                })?;
                self.say(chat, FRESH).await;
            }
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
            Action::Run(text) => self.run(chat, text).await,
        }
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

    async fn run(&mut self, chat: i64, text: String) {
        if let Some(running) = self.chats.get_mut(&chat) {
            running.queue.push_back(text);
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

    fn start(&mut self, chat: i64, text: String) -> oneshot::Sender<()> {
        let (stop, stopped) = oneshot::channel();
        let bot = Arc::clone(&self.bot);
        let options = Arc::clone(&self.options);
        self.tasks.spawn(async move {
            let reply = match turn(&bot, &options, chat, &text, stopped).await {
                Ok(Some(seen)) => seen.reply(),
                Ok(None) => STOPPED.to_string(),
                Err(error) => format!("error: {error}"),
            };
            if let Err(error) = bot.send(chat, &reply).await {
                eprintln!("{error}");
            }
            chat
        });
        stop
    }

    fn next(&mut self, chat: i64) {
        let queued = self
            .chats
            .get_mut(&chat)
            .and_then(|running| running.queue.pop_front());
        let Some(text) = queued else {
            self.chats.remove(&chat);
            return;
        };
        let stop = self.start(chat, text);
        if let Some(running) = self.chats.get_mut(&chat) {
            running.stop = Some(stop);
        }
    }
}

async fn turn(
    bot: &Bot,
    options: &Options,
    chat: i64,
    text: &str,
    mut stopped: oneshot::Receiver<()>,
) -> Result<Option<Seen>> {
    let session = store::load()?.session(chat).map(str::to_string);
    let mut child = tokio::process::Command::new(std::env::current_exe()?)
        .args(argv(options, session.as_deref(), text))
        .current_dir(&options.dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::Tool("the turn has no output".to_string()))?;
    let mut lines = BufReader::new(stdout).lines();
    let mut beat = tokio::time::interval(TYPING_EVERY);
    let mut seen = Seen::default();
    loop {
        tokio::select! {
            Ok(()) = &mut stopped => {
                child.kill().await?;
                return Ok(None);
            }
            _ = beat.tick() => drop(bot.typing(chat).await),
            line = lines.next_line() => match line? {
                Some(line) => seen.read(&line),
                None => break,
            },
        }
    }
    child.wait().await?;
    if let Some(id) = seen.session.clone() {
        store::update(|state| {
            state.set_session(chat, Some(id));
            Ok(())
        })?;
    }
    Ok(Some(seen))
}
