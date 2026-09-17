use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use futures::future::join_all;
use sigil_ide_core::buffer::Rope;
use sigil_ide_core::workspace::{FsEvent, WorkspaceRoot};
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
                    Ok((rope, added, image)) => self.send(Msg::View {
                        file: root.join(&path),
                        path,
                        rope,
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

type Viewed = (Rope, Vec<(usize, usize)>, Option<Vec<u8>>);

fn view(root: &Path, path: &str) -> Result<Viewed> {
    let full = root.join(path);
    let bytes = std::fs::read(&full)?;
    if crate::tools::image_type(&bytes).is_some() {
        return Ok((
            Rope::from_reader(std::io::empty())?,
            Vec::new(),
            Some(bytes),
        ));
    }
    let rope = Rope::from_reader(bytes.as_slice())?;
    let added = diff::hunks(root, path).unwrap_or_default();
    Ok((rope, added, None))
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
