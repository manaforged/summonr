use crate::hub::Fact;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

use notify::Event as NotifyEvent;
use tokio::sync::mpsc::UnboundedSender;

use super::{Cmd, Msg};
use crate::diff;
use crate::endpoint;
use crate::session::prompts;
use crate::worktree;
use summonr_core::dirs::summonr_home;

mod files;
mod forge;
mod load;
mod models;

use files::{add, clipboard, files, relative, unignored, view, watch};
use load::Load;

#[derive(Clone)]
pub(super) struct Reader {
    workspace: PathBuf,
    board: crate::mcp::Board,
    ui: UnboundedSender<Msg>,
    diff_load: Arc<Load>,
    files_load: Arc<Load>,
    panel_load: Arc<Load>,
    trust: bool,
}

impl Reader {
    pub(super) fn new(
        workspace: PathBuf,
        board: crate::mcp::Board,
        ui: UnboundedSender<Msg>,
        trust: bool,
    ) -> Self {
        Self {
            workspace,
            board,
            ui,
            diff_load: Arc::default(),
            files_load: Arc::default(),
            panel_load: Arc::default(),
            trust,
        }
    }

    fn load_slot(&self, command: &Cmd) -> Option<Arc<Load>> {
        if matches!(command, Cmd::Diff(_)) {
            return Some(Arc::clone(&self.diff_load));
        }
        if matches!(command, Cmd::RunPanel(_)) {
            return Some(Arc::clone(&self.panel_load));
        }
        matches!(command, Cmd::ListFiles).then(|| Arc::clone(&self.files_load))
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
        let slot = self.load_slot(&command);
        let first = match &slot {
            Some(slot) => slot.claim(command),
            None => Some(command),
        };
        let Some(command) = first else {
            return;
        };
        let save = matches!(command, Cmd::Save(..));
        let reader = self.clone();
        let task = tokio::task::spawn_blocking(move || {
            reader.serve(command);
            while let Some(next) = slot.as_deref().and_then(Load::next) {
                reader.serve(next);
            }
        });
        if save && let Err(error) = task.await {
            self.send(Msg::Fact(Fact::System(format!(
                "save task failed: {error}"
            ))));
        }
    }

    fn run_panel(&self, name: String) -> Msg {
        let panel = crate::panel::list(&self.workspace, self.trust)
            .into_iter()
            .find(|panel| panel.name == name);
        let result = match panel {
            Some(panel) => tokio::runtime::Handle::current()
                .block_on(crate::panel::run(&panel, &self.workspace)),
            None => Err(format!("no panel named `{name}`")),
        };
        Msg::Fact(Fact::Panel { name, result })
    }

    pub(super) fn serve(&self, command: Cmd) {
        match command {
            Cmd::ListPrompts(session) => {
                let result = prompts(session.as_deref()).map_err(|error| error.to_string());
                self.send(Msg::Fact(Fact::Prompts { session, result }));
            }
            Cmd::ListSessions => {
                let roots = crate::session::roots(&self.workspace);
                let rows = crate::session::list()
                    .map(|rows| {
                        rows.into_iter()
                            .rev()
                            .map(|info| {
                                let local =
                                    info.cwd.as_ref().is_some_and(|cwd| roots.contains(cwd));
                                (info, local)
                            })
                            .collect()
                    })
                    .map_err(|error| error.to_string());
                self.send(Msg::Fact(Fact::Sessions(rows)));
            }
            Cmd::ListUsage => {
                let prices = endpoint::ready(endpoint::Requirement::Key)
                    .0
                    .into_iter()
                    .filter_map(|selection| Some((selection.model, selection.price?)))
                    .collect();
                let roots = crate::session::roots(&self.workspace);
                let rows = summonr_home()
                    .and_then(|home| crate::session::spend(&home, &roots, &prices))
                    .map_err(|error| error.to_string());
                self.send(Msg::Fact(Fact::Usage(rows)));
            }
            Cmd::DeleteSession(id) => {
                match crate::session::remove(&id) {
                    Ok(name) => {
                        self.send(Msg::Fact(Fact::System(format!("deleted session {name}"))))
                    }
                    Err(error) => self.send(Msg::Fact(Fact::Failed(error.to_string()))),
                }
                self.serve(Cmd::ListSessions);
            }
            Cmd::ListFiles => self.send(Msg::Fact(Fact::Files(files(&self.workspace)))),
            Cmd::Jobs => self.jobs(),
            Cmd::Mcp => self.mcp(),
            Cmd::KillJob(id) => self.kill(&id),
            Cmd::Clipboard => self.send(Msg::Fact(Fact::Clipboard(
                clipboard().map_err(|error| error.to_string()),
            ))),
            Cmd::Diff(root) => self.diff(root),
            Cmd::SessionDiff(session) => self.session_diff(&session),
            Cmd::Patch(root, path) => self.patch(root, path),
            Cmd::Review {
                root,
                path,
                hunk,
                op,
            } => self.review(root, path, hunk, op),
            Cmd::Trees => self.trees(),
            Cmd::ListPanels => self.send(Msg::Fact(Fact::Panels(crate::panel::list(
                &self.workspace,
                self.trust,
            )))),
            Cmd::RunPanel(name) => self.send(self.run_panel(name)),
            Cmd::Forge(ask) => self.ship(&ask, ""),
            Cmd::Offers => self.offers(),
            Cmd::Save(file, text) => self.save(file, text),
            Cmd::View(root, path) => self.show(root, path),
            Cmd::Pin(selection, on) => {
                if let Err(error) =
                    summonr_home().and_then(|home| crate::recent::pin(&home, &selection, on))
                {
                    self.send(Msg::Fact(Fact::ModelNote(format!(
                        "pin not saved: {error}"
                    ))));
                }
            }
            Cmd::SaveDefault(selection) => {
                let saved = crate::config::path().and_then(|path| {
                    let pair = selection
                        .as_ref()
                        .map(|selection| (selection.endpoint.as_str(), selection.model.as_str()));
                    crate::config::save_default_model(&path, pair)
                });
                self.send(match saved {
                    Ok(()) => Msg::Fact(Fact::Defaulted(selection)),
                    Err(error) => Msg::Fact(Fact::ModelNote(error.to_string())),
                });
            }
            _ => {}
        }
    }

    fn save(&self, file: PathBuf, text: String) {
        let result = crate::durable::store::stage(&file, text.as_bytes(), true)
            .and_then(crate::durable::store::Staged::commit)
            .map_err(|error| error.to_string());
        self.send(Msg::Fact(Fact::Saved { file, text, result }));
    }

    fn show(&self, root: Option<PathBuf>, path: String) {
        let dir = root.as_deref().unwrap_or(&self.workspace);
        match view(dir, &path) {
            Ok((text, added, image)) => self.send(Msg::Fact(Fact::View {
                file: dir.join(&path),
                root,
                path,
                text,
                added,
                image,
            })),
            Err(error) => self.send(Msg::Fact(Fact::System(format!(
                "cannot view {path}: {error}"
            )))),
        }
    }

    fn patch(&self, root: Option<PathBuf>, path: String) {
        let dir = root.clone().unwrap_or_else(|| self.workspace.clone());
        let result = diff::patch(&dir, (!path.is_empty()).then_some(path.as_str()))
            .map_err(|error| error.to_string());
        self.send(Msg::Fact(Fact::Patch {
            root: root.clone(),
            path,
            result,
        }));
        self.send(Msg::Fact(Fact::Review {
            root,
            result: Ok(crate::hunks::load(&dir)),
        }));
    }

    fn review(&self, root: Option<PathBuf>, path: String, hunk: String, op: crate::hunks::Op) {
        let note = match op {
            crate::hunks::Op::Reject => Some(format!("reverted a hunk in {path}")),
            crate::hunks::Op::RejectFile => Some(format!("reverted {path}")),
            crate::hunks::Op::Restore => Some(format!("restored a hunk in {path}")),
            crate::hunks::Op::Verdict(_) | crate::hunks::Op::Comment(_) => None,
        };
        let dir = root.clone().unwrap_or_else(|| self.workspace.clone());
        let result = crate::hunks::apply(&dir, &path, &hunk, op).map_err(|error| error.to_string());
        if let (Some(note), Ok(_)) = (note, &result) {
            self.send(Msg::Fact(Fact::System(note)));
        }
        self.send(Msg::Fact(Fact::Review { root, result }));
    }

    fn diff(&self, root: Option<PathBuf>) {
        match diff::stats(root.as_deref().unwrap_or(&self.workspace)) {
            Ok(diff) => self.send(Msg::Fact(Fact::Diff(diff, root))),
            Err(error) => self.send(Msg::Fact(Fact::DiffFailed(error.to_string(), root))),
        }
    }

    fn session_diff(&self, session: &str) {
        let diff = diff::turn_stats(&self.workspace, session, diff::TurnId::Session)
            .unwrap_or_else(|_| diff::Diff {
                base: diff::SESSION_BASE.to_string(),
                session: Some(session.to_string()),
                files: Vec::new(),
                strays: Vec::new(),
            });
        self.send(Msg::Fact(Fact::Diff(diff, None)));
    }

    pub(super) fn jobs(&self) {
        let records = crate::jobs::since(&self.workspace, 0, None);
        let jobs = crate::jobs::statuses(&self.workspace, records);
        self.send(Msg::Fact(Fact::Jobs(jobs)));
    }

    fn mcp(&self) {
        match crate::mcp::status(&self.workspace, Some(&self.board)) {
            Ok(servers) => self.send(Msg::Fact(Fact::Mcp(servers))),
            Err(error) => self.send(Msg::Fact(Fact::System(format!("mcp status: {error}")))),
        }
    }

    fn offers(&self) {
        let mut offers = crate::mcp::offers(&self.workspace);
        offers.extend(crate::plugins::commands::offers(&self.workspace));
        self.send(Msg::Fact(Fact::Offers(offers)));
    }

    fn kill(&self, id: &str) {
        match crate::tools::command::kill_job(&self.workspace, id) {
            Ok(status) => self.send(Msg::Fact(Fact::System(format!(
                "job {id}: {}",
                status.label()
            )))),
            Err(error) => self.send(Msg::Fact(Fact::System(format!(
                "job {id} not stopped: {error}"
            )))),
        }
        self.jobs();
    }

    pub(super) fn trees(&self) {
        match worktree::status(&self.workspace) {
            Ok(trees) => self.send(Msg::Fact(Fact::Trees(trees))),
            Err(error) => self.send(Msg::Fact(Fact::System(format!(
                "worktrees unavailable: {error}"
            )))),
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
                self.send(Msg::Fact(Fact::System(format!(
                    "workspace watch unavailable: {error}"
                ))));
                return;
            }
        };
        while let Ok(first) = receiver.recv() {
            let mut events = vec![first];
            let deadline = Instant::now() + crate::limits::limits().reader_window();
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
                    Err(error) => self.send(Msg::Fact(Fact::System(format!(
                        "workspace watch failed: {error}"
                    )))),
                }
            }
            let changed = unignored(&root, paths);
            let dirs = changed.iter().map(|path| root.join(path));
            if let Err(error) = watch(&mut watcher, dirs.filter(|dir| dir.is_dir()).collect()) {
                self.send(Msg::Fact(Fact::System(format!(
                    "workspace watch failed: {error}"
                ))));
            }
            if !changed.is_empty() && self.ui.send(Msg::Fact(Fact::Changed(changed))).is_err() {
                return;
            }
        }
    }
}
