use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

const AGENT: &str = "summonr";
const SOURCE: &str = "summonr:lifecycle";
const DISPLAY: &str = "summonr:display";

pub(crate) struct Reporter {
    target: Option<Arc<Target>>,
}

pub(crate) struct Turn {
    target: Option<Arc<Target>>,
}

struct Target {
    bin: OsString,
    pane: OsString,
    socket: OsString,
    session: Mutex<String>,
    title: Mutex<String>,
    model: Mutex<String>,
    life: AtomicU64,
    meta: AtomicU64,
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
            });
        if let Some(target) = &target {
            target.report("idle");
            target.metadata();
        }
        Self { target }
    }

    pub(crate) fn turn(&self) -> Turn {
        if let Some(target) = &self.target {
            target.report("working");
        }
        Turn {
            target: self.target.clone(),
        }
    }

    pub(crate) fn set_model(&self, model: &str) {
        if let Some(target) = &self.target {
            store(&target.model, short_model(model));
            target.metadata();
        }
    }

    pub(crate) fn retarget(&self, session: &str) {
        if let Some(target) = &self.target {
            target.set_session(session);
            target.report("idle");
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

impl Drop for Turn {
    fn drop(&mut self) {
        if let Some(target) = &self.target {
            target.report("idle");
        }
    }
}

impl Target {
    fn report(&self, state: &str) {
        let seq = self.life.fetch_add(1, Ordering::Relaxed) + 1;
        self.run(report_args(&self.pane, state, &held(&self.session), seq));
    }

    fn metadata(&self) {
        let seq = self.meta.fetch_add(1, Ordering::Relaxed) + 1;
        let title = held(&self.title);
        let model = held(&self.model);
        self.run(metadata_args(&self.pane, &title, &model, seq));
    }

    fn set_session(&self, session: &str) {
        store(&self.session, session.to_string());
    }

    fn release(&self) {
        let seq = self.life.fetch_add(1, Ordering::Relaxed) + 1;
        self.run(release_args(&self.pane, seq));
    }

    fn run(&self, args: Vec<OsString>) {
        let mut command = Command::new(&self.bin);
        command
            .args(args)
            .env("HERDR_SOCKET_PATH", &self.socket)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        drop(command.status());
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
        pane,
        socket,
        session: Mutex::new(session.to_string()),
        title: Mutex::new(folder(workspace)),
        model: Mutex::new(short_model(model)),
        life: AtomicU64::new(0),
        meta: AtomicU64::new(0),
    })
}

fn held(value: &Mutex<String>) -> String {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

fn store(value: &Mutex<String>, next: String) {
    let mut slot = value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
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

fn report_args(pane: &OsStr, state: &str, session: &str, seq: u64) -> Vec<OsString> {
    [
        OsString::from("pane"),
        OsString::from("report-agent"),
        pane.to_os_string(),
        OsString::from("--source"),
        OsString::from(SOURCE),
        OsString::from("--agent"),
        OsString::from(AGENT),
        OsString::from("--state"),
        OsString::from(state),
        OsString::from("--agent-session-id"),
        OsString::from(session),
        OsString::from("--seq"),
        OsString::from(seq.to_string()),
    ]
    .into()
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

fn short_model(model: &str) -> String {
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
