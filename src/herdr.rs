use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;

const AGENT: &str = "summoner";
const SOURCE: &str = "summoner:lifecycle";

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
    session: String,
}

impl Reporter {
    pub(crate) fn discover(session: &str, depth: u8) -> Self {
        let target = (depth == 0)
            .then(|| std::env::current_exe().ok())
            .flatten()
            .and_then(|executable| {
                target(session, product(&executable), |key| std::env::var_os(key)).map(Arc::new)
            });
        if let Some(target) = &target {
            target.report("idle");
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
        self.run(report_args(&self.pane, state, &self.session));
    }

    fn release(&self) {
        self.run(release_args(&self.pane));
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
    executable: Option<&OsStr>,
    env: impl Fn(&str) -> Option<OsString>,
) -> Option<Target> {
    if env("HERDR_ENV").as_deref() != Some(OsStr::new("1"))
        || !matches!(executable, Some(name) if name == "summon" || name == "summoner")
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
        session: session.to_string(),
    })
}

fn product(executable: &Path) -> Option<&OsStr> {
    let name = executable.file_stem()?;
    let test = executable.components().any(|part| {
        let part = part.as_os_str();
        part == "deps" || part.to_string_lossy().starts_with("test-")
    });
    (!test && matches!(name.to_str(), Some("summon" | "summoner"))).then_some(name)
}

fn present(value: Option<OsString>) -> Option<OsString> {
    value.filter(|value| !value.is_empty())
}

fn report_args(pane: &OsStr, state: &str, session: &str) -> Vec<OsString> {
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
    ]
    .into()
}

fn release_args(pane: &OsStr) -> Vec<OsString> {
    [
        OsString::from("pane"),
        OsString::from("release-agent"),
        pane.to_os_string(),
        OsString::from("--source"),
        OsString::from(SOURCE),
        OsString::from("--agent"),
        OsString::from(AGENT),
    ]
    .into()
}

#[cfg(test)]
#[path = "herdr_tests.rs"]
mod tests;
