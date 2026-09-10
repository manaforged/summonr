use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use sigil_ide_core::buffer::Rope;
use sigil_ide_core::workspace::{FsEvent, WorkspaceRoot};
use tokio::sync::mpsc::UnboundedSender;

use super::{Cmd, Msg};
use crate::diff;
use crate::error::Result;
use crate::session::prompts;
use crate::worktree;

const WINDOW: Duration = Duration::from_millis(400);

#[derive(Clone)]
pub(super) struct Reader {
    workspace: PathBuf,
    ui: UnboundedSender<Msg>,
}

impl Reader {
    pub(super) fn new(workspace: PathBuf, ui: UnboundedSender<Msg>) -> Self {
        Self { workspace, ui }
    }

    pub(super) async fn run(&self, command: Cmd) {
        let save = matches!(command, Cmd::Save(..));
        let reader = self.clone();
        let task = tokio::task::spawn_blocking(move || reader.serve(command));
        if save && let Err(error) = task.await {
            self.send(Msg::System(format!("save task failed: {error}")));
        }
    }

    pub(super) fn serve(&self, command: Cmd) {
        match command {
            Cmd::ListPrompts(session) => {
                let result = prompts(session.as_deref()).map_err(|error| error.to_string());
                self.send(Msg::Prompts { session, result });
            }
            Cmd::ListSessions => {
                let roots = crate::session::roots(&self.workspace);
                let rows = crate::session::list()
                    .map(|rows| {
                        rows.into_iter()
                            .rev()
                            .filter(|info| info.depth == 0)
                            .map(|info| {
                                let local =
                                    info.cwd.as_ref().is_some_and(|cwd| roots.contains(cwd));
                                (info, local)
                            })
                            .collect()
                    })
                    .map_err(|error| error.to_string());
                self.send(Msg::Sessions(rows));
            }
            Cmd::DeleteSession(id) => {
                match crate::session::remove(&id) {
                    Ok(name) => self.send(Msg::System(format!("deleted session {name}"))),
                    Err(error) => self.send(Msg::Failed(error.to_string())),
                }
                self.serve(Cmd::ListSessions);
            }
            Cmd::ListFiles => self.send(Msg::Files(files(&self.workspace))),
            Cmd::Diff(root) => self.diff(root.as_deref().unwrap_or(&self.workspace)),
            Cmd::Patch(root, path) => {
                let root = root.as_deref().unwrap_or(&self.workspace);
                let text = diff::patch(root, Some(&path))
                    .unwrap_or_else(|error| format!("diff failed: {error}"));
                self.send(Msg::Patch { path, text });
            }
            Cmd::Trees => self.trees(),
            Cmd::Save(file, text) => {
                let result = std::fs::write(&file, &text).map_err(|error| error.to_string());
                self.send(Msg::Saved { file, text, result });
            }
            Cmd::View(root, path) => {
                let root = root.as_deref().unwrap_or(&self.workspace);
                match view(root, &path) {
                    Ok((rope, added)) => self.send(Msg::View {
                        file: root.join(&path),
                        path,
                        rope,
                        added,
                    }),
                    Err(error) => self.send(Msg::System(format!("cannot view {path}: {error}"))),
                }
            }
            _ => {}
        }
    }

    pub(super) fn diff(&self, root: &Path) {
        match diff::stats(root) {
            Ok(diff) => self.send(Msg::Diff(diff)),
            Err(error) => self.send(Msg::System(format!("diff unavailable: {error}"))),
        }
    }

    pub(super) fn trees(&self) {
        match worktree::status(&self.workspace) {
            Ok(trees) => self.send(Msg::Trees(trees)),
            Err(error) => self.send(Msg::System(format!("worktrees unavailable: {error}"))),
        }
    }

    fn send(&self, msg: Msg) {
        drop(self.ui.send(msg));
    }

    pub(super) fn watch(&self) {
        let reader = self.clone();
        drop(
            thread::Builder::new()
                .name("summon-watch".into())
                .spawn(move || reader.forward()),
        );
    }

    fn forward(&self) {
        let Ok(root) = WorkspaceRoot::open(&self.workspace) else {
            return;
        };
        let Ok(watcher) = root.watch() else {
            return;
        };
        while let Some(first) = watcher.wait() {
            let mut events = vec![first];
            let deadline = Instant::now() + WINDOW;
            while let Some(more) =
                watcher.wait_timeout(deadline.saturating_duration_since(Instant::now()))
            {
                events.push(more);
                if Instant::now() >= deadline {
                    break;
                }
            }
            let paths: Vec<String> = events
                .iter()
                .map(|event| relative(root.path(), event))
                .filter(|path| !path.is_empty())
                .collect();
            let changed = unignored(&self.workspace, paths);
            if !changed.is_empty() && self.ui.send(Msg::Changed(changed)).is_err() {
                return;
            }
        }
    }
}

fn files(workspace: &Path) -> Vec<String> {
    let Ok(root) = WorkspaceRoot::open(workspace) else {
        return Vec::new();
    };
    let base = root.path().to_path_buf();
    root.files()
        .filter_map(|path| path.strip_prefix(&base).ok().map(text))
        .collect()
}

fn view(root: &Path, path: &str) -> Result<(Rope, Vec<(usize, usize)>)> {
    let file = std::fs::File::open(root.join(path))?;
    let rope = Rope::from_reader(file)?;
    let added = diff::hunks(root, path).unwrap_or_default();
    Ok((rope, added))
}

fn relative(root: &Path, event: &FsEvent) -> String {
    let path = match event {
        FsEvent::Created(path) | FsEvent::Modified(path) | FsEvent::Removed(path) => path,
        FsEvent::Renamed(_, to) => to,
    };
    path.strip_prefix(root).map(text).unwrap_or_default()
}

fn text(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn unignored(workspace: &Path, mut paths: Vec<String>) -> Vec<String> {
    paths.sort();
    paths.dedup();
    paths.retain(|path| !path.split('/').any(|part| part.starts_with('.')));
    if paths.is_empty() {
        return paths;
    }
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace)
        .args(["check-ignore", "--stdin", "-z"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .and_then(|mut child| {
            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write as _;
                for path in &paths {
                    drop(stdin.write_all(path.as_bytes()));
                    drop(stdin.write_all(b"\0"));
                }
            }
            child.wait_with_output()
        });
    let Ok(output) = output else {
        return paths;
    };
    if output.status.code() == Some(128) {
        return paths;
    }
    let ignored: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect();
    paths.retain(|path| !ignored.contains(path));
    paths
}

#[cfg(test)]
#[path = "reader_tests.rs"]
mod tests;
