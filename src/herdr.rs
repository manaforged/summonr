use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

const AGENT: &str = "summonr";
const SOURCE: &str = "summonr:lifecycle";
const DISPLAY: &str = "summonr:display";

static CLAIMED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Idle,
    Working,
    Blocked,
}

impl State {
    const fn wire(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Working => "working",
            Self::Blocked => "blocked",
        }
    }
}

pub(crate) struct Reporter {
    target: Option<Arc<Target>>,
}

pub(crate) struct Turn {
    target: Option<Arc<Target>>,
}

pub(crate) struct Blocked {
    target: Option<Arc<Target>>,
}

struct Target {
    bin: OsString,
    command: OsString,
    pane: OsString,
    socket: OsString,
    session: Mutex<String>,
    title: Mutex<String>,
    model: Mutex<String>,
    life: AtomicU64,
    meta: AtomicU64,
    queue: Mutex<Option<Sender<Vec<OsString>>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl Reporter {
    pub(crate) fn discover(session: &str, workspace: &Path, model: &str, depth: u8) -> Self {
        let target = (depth == 0)
            .then(|| std::env::current_exe().ok())
            .flatten()
            .and_then(|executable| {
                target(session, workspace, model, product(&executable), |key| {
                    std::env::var_os(key)
                })
                .map(Arc::new)
            })
            .filter(|_| !CLAIMED.swap(true, Ordering::Relaxed));
        if let Some(target) = &target {
            target.report(State::Idle);
            target.metadata();
        }
        Self { target }
    }

    pub(crate) fn turn(&self) -> Turn {
        if let Some(target) = &self.target {
            target.report(State::Working);
        }
        Turn {
            target: self.target.clone(),
        }
    }

    pub(crate) fn blocked(&self, message: &str) -> Blocked {
        if let Some(target) = &self.target {
            target.report_with(State::Blocked, Some(message));
        }
        Blocked {
            target: self.target.clone(),
        }
    }

    pub(crate) fn set_model(&self, model: &str) {
        if let Some(target) = &self.target {
            store(&target.model, model_label(model));
            target.metadata();
        }
    }

    pub(crate) fn retarget(&self, session: &str) {
        if let Some(target) = &self.target {
            target.set_session(session);
            target.report(State::Idle);
            target.metadata();
        }
    }
}

impl Drop for Reporter {
    fn drop(&mut self) {
        if let Some(target) = &self.target {
            target.release();
        }
    }
}

impl Drop for Blocked {
    fn drop(&mut self) {
        if let Some(target) = &self.target {
            target.report(State::Working);
        }
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        if let Some(target) = &self.target {
            target.report(State::Idle);
        }
    }
}

impl Target {
    fn report(&self, state: State) {
        self.report_with(state, None);
    }

    fn report_with(&self, state: State, message: Option<&str>) {
        let seq = next_seq(&self.life);
        let report = Report {
            state,
            message,
            session: &held(&self.session),
            command: &self.command,
            seq,
        };
        self.run(report_args(&self.pane, &report));
    }

    fn metadata(&self) {
        let seq = next_seq(&self.meta);
        let title = held(&self.title);
        let model = held(&self.model);
        self.run(metadata_args(&self.pane, &title, &model, seq));
    }

    fn set_session(&self, session: &str) {
        store(&self.session, session.to_string());
    }

    fn release(&self) {
        let seq = next_seq(&self.life);
        self.run(release_args(&self.pane, seq));
        drop(
            self.queue
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take(),
        );
        let worker = self
            .worker
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(worker) = worker {
            drop(worker.join());
        }
    }

    fn run(&self, args: Vec<OsString>) {
        let mut queue = self.queue.lock().unwrap_or_else(PoisonError::into_inner);
        if queue.is_none() {
            *queue = self.spawn();
        }
        if let Some(queue) = queue.as_ref() {
            drop(queue.send(args));
        }
    }

    fn spawn(&self) -> Option<Sender<Vec<OsString>>> {
        let (send, receive) = std::sync::mpsc::channel::<Vec<OsString>>();
        let bin = self.bin.clone();
        let socket = self.socket.clone();
        let worker = std::thread::Builder::new()
            .name("herdr".to_string())
            .spawn(move || {
                for args in receive {
                    drop(
                        Command::new(&bin)
                            .args(args)
                            .env("HERDR_SOCKET_PATH", &socket)
                            .stdin(Stdio::null())
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .status(),
                    );
                }
            })
            .ok()?;
        *self.worker.lock().unwrap_or_else(PoisonError::into_inner) = Some(worker);
        Some(send)
    }
}

fn target(
    session: &str,
    workspace: &Path,
    model: &str,
    executable: Option<&OsStr>,
    env: impl Fn(&str) -> Option<OsString>,
) -> Option<Target> {
    if env("HERDR_ENV").as_deref() != Some(OsStr::new("1"))
        || !matches!(executable, Some(name) if name == "summon" || name == "summoner" || name == "summonr")
    {
        return None;
    }
    let pane = present(env("HERDR_PANE_ID"))?;
    let socket = present(env("HERDR_SOCKET_PATH"))?;
    let bin = present(env("HERDR_BIN_PATH")).unwrap_or_else(|| OsString::from("herdr"));
    Some(Target {
        bin,
        command: executable?.to_os_string(),
        pane,
        socket,
        session: Mutex::new(session.to_string()),
        title: Mutex::new(folder(workspace)),
        model: Mutex::new(model_label(model)),
        life: AtomicU64::new(0),
        meta: AtomicU64::new(0),
        queue: Mutex::new(None),
        worker: Mutex::new(None),
    })
}

fn held(value: &Mutex<String>) -> String {
    value.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

fn store(value: &Mutex<String>, next: String) {
    let mut slot = value.lock().unwrap_or_else(PoisonError::into_inner);
    slot.clear();
    slot.push_str(&next);
}

fn product(executable: &Path) -> Option<&OsStr> {
    let name = executable.file_stem()?;
    let test = executable.components().any(|part| {
        let part = part.as_os_str();
        part == "deps" || part.to_string_lossy().starts_with("test-")
    });
    (!test && matches!(name.to_str(), Some("summon" | "summoner" | "summonr"))).then_some(name)
}

fn present(value: Option<OsString>) -> Option<OsString> {
    value.filter(|value| !value.is_empty())
}

struct Report<'a> {
    state: State,
    message: Option<&'a str>,
    session: &'a str,
    command: &'a OsStr,
    seq: u64,
}

fn next_seq(counter: &AtomicU64) -> u64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        });
    let mut last = counter.load(Ordering::Relaxed);
    loop {
        let next = now.max(last + 1);
        match counter.compare_exchange(last, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return next,
            Err(seen) => last = seen,
        }
    }
}

fn report_args(pane: &OsStr, report: &Report<'_>) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["pane", "report-agent"].map(OsString::from).into();
    args.push(pane.to_os_string());
    for (flag, value) in [
        ("--source", SOURCE),
        ("--agent", AGENT),
        ("--state", report.state.wire()),
    ] {
        args.extend([OsString::from(flag), OsString::from(value)]);
    }
    if let Some(message) = report.message {
        args.extend([
            OsString::from("--message"),
            OsString::from(crate::text::ellipsize(message.trim(), 160)),
        ]);
    }
    args.extend([
        OsString::from("--seq"),
        OsString::from(report.seq.to_string()),
    ]);
    if !report.session.is_empty() {
        args.extend([
            OsString::from("--agent-session-id"),
            OsString::from(report.session),
            OsString::from("--"),
            report.command.to_os_string(),
            OsString::from("resume"),
            OsString::from(report.session),
        ]);
    }
    args
}

fn metadata_args(pane: &OsStr, title: &str, model: &str, seq: u64) -> Vec<OsString> {
    let shown = if model.is_empty() {
        AGENT.to_string()
    } else {
        format!("{AGENT} - {model}")
    };
    [
        OsString::from("pane"),
        OsString::from("report-metadata"),
        pane.to_os_string(),
        OsString::from("--source"),
        OsString::from(DISPLAY),
        OsString::from("--agent"),
        OsString::from(AGENT),
        OsString::from("--title"),
        OsString::from(crate::text::ellipsize(title.trim(), 80)),
        OsString::from("--display-agent"),
        OsString::from(shown),
        OsString::from("--seq"),
        OsString::from(seq.to_string()),
    ]
    .into()
}

fn model_label(model: &str) -> String {
    let name = model.rsplit(['/', ':']).next().unwrap_or(model);
    crate::text::ellipsize(name.trim(), 80)
}

fn folder(workspace: &Path) -> String {
    workspace
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| crate::text::ellipsize(name.trim(), 80))
        .unwrap_or_default()
}

fn release_args(pane: &OsStr, seq: u64) -> Vec<OsString> {
    [
        OsString::from("pane"),
        OsString::from("release-agent"),
        pane.to_os_string(),
        OsString::from("--source"),
        OsString::from(SOURCE),
        OsString::from("--agent"),
        OsString::from(AGENT),
        OsString::from("--seq"),
        OsString::from(seq.to_string()),
    ]
    .into()
}

#[cfg(all(test, unix))]
#[path = "herdr_tests.rs"]
mod tests;
