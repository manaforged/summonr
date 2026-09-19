use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use futures::future::join_all;
use ignore::WalkBuilder;
use notify::event::{EventKind, ModifyKind};
use notify::{Event as NotifyEvent, RecursiveMode, Watcher};
use tokio::sync::mpsc::UnboundedSender;

use super::{Cmd, Msg};
use crate::diff;
use crate::endpoint;
use crate::error::Result;
use crate::prompt::mage_home;
use crate::provider::Client;
use crate::recent;
use crate::session::prompts;
use crate::worktree;

const WINDOW: Duration = Duration::from_millis(400);
const PROBE_TIMEOUT: Duration = Duration::from_secs(8);

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
        if let Cmd::ListModels { request, cancel } = command {
            let reader = self.clone();
            drop(tokio::spawn(async move {
                tokio::select! {
                    biased;
                    _ = cancel => {}
                    _ = reader.ui.closed() => {}
                    () = reader.models(request) => {}
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
                let prices = endpoint::ready(false)
                    .0
                    .into_iter()
                    .filter_map(|seat| Some((seat.model, seat.price?)))
                    .collect();
                let roots = crate::session::roots(&self.workspace);
                let rows = mage_home()
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
            Cmd::ListFiles => self.send(Msg::Files(files(&self.workspace))),
            Cmd::Diff(root) => self.diff(root.as_deref().unwrap_or(&self.workspace)),
            Cmd::Patch(root, path) => {
                let result = diff::patch(
                    root.as_deref().unwrap_or(&self.workspace),
                    (!path.is_empty()).then_some(path.as_str()),
                )
                .map_err(|error| error.to_string());
                self.send(Msg::Patch { root, path, result });
            }
            Cmd::Trees => self.trees(),
            Cmd::Save(file, text) => {
                let result = std::fs::write(&file, &text).map_err(|error| error.to_string());
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
            _ => {}
        }
    }

    pub(super) fn diff(&self, root: &Path) {
        match diff::stats(root) {
            Ok(diff) => self.send(Msg::Diff(diff)),
            Err(error) => self.send(Msg::DiffFailed(error.to_string())),
        }
    }

    async fn models(&self, request: u64) {
        let (endpoints, mut errors) = endpoint::ready(false);
        let mut rows = mage_home()
            .map(|home| recent::load(&home))
            .unwrap_or_default();
        rows.retain(|row| {
            row.split_once('\t')
                .is_some_and(|(name, _)| endpoints.iter().any(|endpoint| endpoint.name == name))
        });
        let recent = rows.len();
        for endpoint in endpoints
            .iter()
            .filter(|endpoint| !endpoint.model.is_empty())
        {
            let row = format!("{}\t{}", endpoint.name, endpoint.model);
            if !rows.contains(&row) {
                rows.push(row);
            }
        }
        self.send(Msg::Models {
            request,
            recent,
            rows: rows.clone(),
            loading: !endpoints.is_empty(),
            error: if endpoints.is_empty() && errors.is_empty() {
                "No configured providers. Use /login to connect one.".to_string()
            } else {
                errors.join("; ")
            },
        });
        if endpoints.is_empty() {
            return;
        }
        let catalogs = join_all(endpoints.into_iter().map(|endpoint| async move {
            let fetch = async {
                let client = Client::connect(&endpoint)?;
                client.models().await
            };
            let result = match tokio::time::timeout(PROBE_TIMEOUT, fetch).await {
                Ok(result) => result.map_err(|error| error.to_string()),
                Err(_) => Err("model discovery timed out".to_string()),
            };
            (endpoint.name, result)
        }))
        .await;
        for (name, result) in catalogs {
            match result {
                Ok(models) if models.is_empty() => {
                    errors.push(format!("{name}: no models returned"))
                }
                Ok(models) => {
                    for model in models {
                        let row = format!("{name}\t{model}");
                        if !rows.contains(&row) {
                            rows.push(row);
                        }
                    }
                }
                Err(error) => errors.push(format!("{name}: {error}")),
            }
        }
        self.send(Msg::Models {
            request,
            recent,
            rows,
            loading: false,
            error: errors.join("; "),
        });
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
        let root = match std::fs::canonicalize(&self.workspace) {
            Ok(root) => root,
            Err(error) => {
                self.send(Msg::System(format!("workspace watch unavailable: {error}")));
                return;
            }
        };
        let (sender, receiver) = mpsc::channel();
        let ui = self.ui.clone();
        let mut watcher = match notify::recommended_watcher(
            move |result: std::result::Result<NotifyEvent, notify::Error>| match result {
                Ok(event) => drop(sender.send(event)),
                Err(error) => {
                    drop(ui.send(Msg::System(format!("workspace watch failed: {error}"))))
                }
            },
        ) {
            Ok(watcher) => watcher,
            Err(error) => {
                self.send(Msg::System(format!("workspace watch unavailable: {error}")));
                return;
            }
        };
        if let Err(error) = watcher.watch(&root, RecursiveMode::Recursive) {
            self.send(Msg::System(format!("workspace watch unavailable: {error}")));
            return;
        }
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
            let paths: Vec<String> = events
                .into_iter()
                .flat_map(|event| relative(&root, &event))
                .collect();
            let changed = unignored(&self.workspace, paths);
            if !changed.is_empty() && self.ui.send(Msg::Changed(changed)).is_err() {
                return;
            }
        }
    }
}

fn files(workspace: &Path) -> Vec<String> {
    let Ok(root) = std::fs::canonicalize(workspace) else {
        return Vec::new();
    };
    WalkBuilder::new(&root)
        .require_git(false)
        .build()
        .filter_map(std::result::Result::ok)
        .filter_map(|entry| {
            let file_type = entry.file_type()?;
            (file_type.is_file() || file_type.is_symlink())
                .then(|| entry.path().strip_prefix(&root).ok().map(text))
                .flatten()
        })
        .collect()
}

type Viewed = (String, Vec<(usize, usize)>, Option<Vec<u8>>);

fn view(root: &Path, path: &str) -> Result<Viewed> {
    let full = root.join(path);
    let bytes = std::fs::read(&full)?;
    if crate::tools::image_type(&bytes).is_some() {
        return Ok((String::new(), Vec::new(), Some(bytes)));
    }
    let text = String::from_utf8(bytes)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let added = diff::hunks(root, path).unwrap_or_default();
    Ok((text, added, None))
}

fn relative(root: &Path, event: &NotifyEvent) -> Vec<String> {
    let paths = if matches!(&event.kind, EventKind::Modify(ModifyKind::Name(_))) {
        event.paths.iter().rev().take(1).collect::<Vec<_>>()
    } else {
        event.paths.iter().collect()
    };
    paths
        .into_iter()
        .filter_map(|path| path.strip_prefix(root).ok().map(text))
        .filter(|path| !path.is_empty())
        .collect()
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
    let joined: Vec<u8> = paths
        .iter()
        .flat_map(|path| path.bytes().chain(std::iter::once(0)))
        .collect();
    let Ok(output) = crate::diff::git_output(
        workspace,
        &[],
        &["check-ignore", "--stdin", "-z"],
        Some(&joined),
    ) else {
        return paths;
    };
    let ignored: Vec<String> = String::from_utf8_lossy(&output)
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect();
    paths.retain(|path| !ignored.contains(path));
    paths
}
