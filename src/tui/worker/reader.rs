use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

use notify::Event as NotifyEvent;
use tokio::sync::mpsc::UnboundedSender;

use super::{Cmd, Msg};
use crate::diff;
use crate::endpoint;
use crate::prompt::summonr_home;
use crate::session::prompts;
use crate::worktree;

mod files;
mod models;

use files::{add, clipboard, files, relative, unignored, view, watch};

use crate::limits::READER_WINDOW as WINDOW;

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
        if let Cmd::ListModels {
            request,
            only,
            cancel,
        } = command
        {
            let reader = self.clone();
            drop(tokio::spawn(async move {
                tokio::select! {
                    biased;
                    _ = cancel => {}
                    _ = reader.ui.closed() => {}
                    () = reader.models(request, only) => {}
                }
            }));
            return;
        }
        let save = matches!(command, Cmd::Save(..));
        let reader = self.clone();
        let task = tokio::task::spawn_blocking(move || reader.serve(command));
        if save && let Err(error) = task.await {
            self.send(Msg::System(format!("save task failed: {error}")));
        }
    }

    pub(super) fn serve(&self, command: Cmd) {
        match command {
            command @ (Cmd::ListPrompts(_)
            | Cmd::ListSessions
            | Cmd::ListUsage
            | Cmd::DeleteSession(_)) => self.sessions(command),
            command @ (Cmd::ListFiles
            | Cmd::Jobs
            | Cmd::KillJob(_)
            | Cmd::Clipboard
            | Cmd::Diff(_)
            | Cmd::Patch(..)
            | Cmd::Review { .. }
            | Cmd::Trees
            | Cmd::Save(..)
            | Cmd::View(..)) => self.panes(command),
            command @ (Cmd::Pin(..) | Cmd::SaveDefault(_)) => self.seats(command),
            _ => {}
        }
    }

    fn sessions(&self, command: Cmd) {
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
            Cmd::ListUsage => {
                let prices = endpoint::ready(endpoint::Need::Key)
                    .0
                    .into_iter()
                    .filter_map(|seat| Some((seat.model, seat.price?)))
                    .collect();
                let roots = crate::session::roots(&self.workspace);
                let rows = summonr_home()
                    .and_then(|home| crate::session::spend(&home, &roots, &prices))
                    .map_err(|error| error.to_string());
                self.send(Msg::Usage(rows));
            }
            Cmd::DeleteSession(id) => {
                match crate::session::remove(&id) {
                    Ok(name) => self.send(Msg::System(format!("deleted session {name}"))),
                    Err(error) => self.send(Msg::Failed(error.to_string())),
                }
                self.serve(Cmd::ListSessions);
            }
            _command => unreachable!("serve routes only session listings"),
        }
    }

    fn panes(&self, command: Cmd) {
        match command {
            Cmd::ListFiles => self.send(Msg::Files(files(&self.workspace))),
            Cmd::Jobs => self.jobs(),
            Cmd::KillJob(id) => self.kill(&id),
            Cmd::Clipboard => self.send(Msg::Clipboard(
                clipboard().map_err(|error| error.to_string()),
            )),
            Cmd::Diff(root) => self.diff(root.as_deref().unwrap_or(&self.workspace)),
            Cmd::Patch(root, path) => self.patch(root, path),
            Cmd::Review {
                root,
                path,
                hunk,
                op,
            } => self.review(root, path, hunk, op),
            Cmd::Trees => self.trees(),
            Cmd::Save(file, text) => {
                let result = crate::durable::store::stage(&file, text.as_bytes(), true)
                    .and_then(crate::durable::store::Staged::commit)
                    .map_err(|error| error.to_string());
                self.send(Msg::Saved { file, text, result });
            }
            Cmd::View(root, path) => {
                let root = root.as_deref().unwrap_or(&self.workspace);
                match view(root, &path) {
                    Ok((text, added, image)) => self.send(Msg::View {
                        file: root.join(&path),
                        path,
                        text,
                        added,
                        image,
                    }),
                    Err(error) => self.send(Msg::System(format!("cannot view {path}: {error}"))),
                }
            }
            _command => unreachable!("serve routes only pane commands"),
        }
    }

    fn patch(&self, root: Option<PathBuf>, path: String) {
        let dir = root.clone().unwrap_or_else(|| self.workspace.clone());
        let result = diff::patch(&dir, (!path.is_empty()).then_some(path.as_str()))
            .map_err(|error| error.to_string());
        self.send(Msg::Patch {
            root: root.clone(),
            path,
            result,
        });
        self.send(Msg::Review {
            root,
            result: Ok(crate::hunks::load(&dir)),
        });
    }

    fn review(&self, root: Option<PathBuf>, path: String, hunk: String, op: crate::hunks::Op) {
        let rejected = matches!(op, crate::hunks::Op::Reject);
        let dir = root.clone().unwrap_or_else(|| self.workspace.clone());
        let result = crate::hunks::apply(&dir, &path, &hunk, op).map_err(|error| error.to_string());
        if rejected && result.is_ok() {
            self.send(Msg::System(format!("reverted a hunk in {path}")));
        }
        self.send(Msg::Review { root, result });
    }

    fn seats(&self, command: Cmd) {
        match command {
            Cmd::Pin(seat, on) => {
                if let Err(error) = summonr_home()
                    .and_then(|home| crate::recent::pin(&home, &seat, on).map_err(Into::into))
                {
                    self.send(Msg::ModelNote(format!("pin not saved: {error}")));
                }
            }
            Cmd::SaveDefault(seat) => {
                let saved = crate::config::path().and_then(|path| {
                    let pair = seat
                        .as_ref()
                        .map(|seat| (seat.endpoint.as_str(), seat.model.as_str()));
                    crate::config::save_default_seat(&path, pair)
                });
                self.send(match saved {
                    Ok(()) => Msg::Defaulted(seat),
                    Err(error) => Msg::ModelNote(error.to_string()),
                });
            }
            _command => unreachable!("serve routes only seat commands"),
        }
    }

    pub(super) fn diff(&self, root: &Path) {
        match diff::stats(root) {
            Ok(diff) => self.send(Msg::Diff(diff)),
            Err(error) => self.send(Msg::DiffFailed(error.to_string())),
        }
    }

    pub(super) fn jobs(&self) {
        let jobs = crate::jobs::since(&self.workspace, 0, None)
            .into_iter()
            .filter_map(|record| {
                crate::jobs::status(&self.workspace, &record.id)
                    .ok()
                    .map(|status| (record, status))
            })
            .collect();
        self.send(Msg::Jobs(jobs));
    }

    fn kill(&self, id: &str) {
        match crate::tools::command::kill_job(&self.workspace, id) {
            Ok(status) => self.send(Msg::System(format!("job {id}: {}", status.label()))),
            Err(error) => self.send(Msg::System(format!("job {id} not stopped: {error}"))),
        }
        self.jobs();
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
        let (sender, receiver) = mpsc::channel::<notify::Result<NotifyEvent>>();
        let started = std::fs::canonicalize(&self.workspace)
            .map_err(notify::Error::io)
            .and_then(|root| {
                let mut watcher = notify::recommended_watcher(sender)?;
                watch(&mut watcher, vec![root.clone()])?;
                Ok((root, watcher))
            });
        let (root, mut watcher) = match started {
            Ok(started) => started,
            Err(error) => {
                self.send(Msg::System(format!("workspace watch unavailable: {error}")));
                return;
            }
        };
        while let Ok(first) = receiver.recv() {
            let mut events = vec![first];
            let deadline = Instant::now() + WINDOW;
            while let Ok(more) =
                receiver.recv_timeout(deadline.saturating_duration_since(Instant::now()))
            {
                events.push(more);
                if Instant::now() >= deadline {
                    break;
                }
            }
            let mut paths = Vec::new();
            for event in events {
                match event {
                    Ok(event) => paths.extend(relative(&root, &event)),
                    Err(error) => {
                        self.send(Msg::System(format!("workspace watch failed: {error}")))
                    }
                }
            }
            let changed = unignored(&root, paths);
            let dirs = changed.iter().map(|path| root.join(path));
            if let Err(error) = watch(&mut watcher, dirs.filter(|dir| dir.is_dir()).collect()) {
                self.send(Msg::System(format!("workspace watch failed: {error}")));
            }
            if !changed.is_empty() && self.ui.send(Msg::Changed(changed)).is_err() {
                return;
            }
        }
    }
}
