use std::collections::BTreeMap;
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
use crate::prompt::summonr_home;
use crate::provider::Client;
use crate::recent;
use crate::session::prompts;
use crate::worktree;

const WINDOW: Duration = Duration::from_millis(400);
const PROBE_TIMEOUT: Duration = Duration::from_secs(8);
pub(crate) const NEEDS_LOGIN: &str = "needs login";

fn waiting(ready: &[endpoint::Endpoint]) -> Vec<(String, String)> {
    let Ok(names) = endpoint::names() else {
        return Vec::new();
    };
    names
        .into_iter()
        .filter(|name| !ready.iter().any(|endpoint| &endpoint.name == name))
        .filter(|name| {
            endpoint::auth_info(name).is_ok_and(|info| info.flow.signs_in() && !info.configured)
        })
        .filter_map(|name| {
            let endpoint = endpoint::resolve_unauthed(Some(&name), None).ok()?;
            (!endpoint.model.is_empty()).then_some((name, endpoint.model))
        })
        .collect()
}

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
        let mut rows = summonr_home()
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
            add(&mut rows, format!("{}\t{}", endpoint.name, endpoint.model));
        }
        let mut facts = BTreeMap::new();
        let mut groups = BTreeMap::new();
        let kept = crate::catalog::cached();
        for endpoint in &endpoints {
            let listing = kept.get(&endpoint.name);
            groups.insert(
                endpoint.name.clone(),
                match listing {
                    Some(listing) if listing.stale() => "cached".to_string(),
                    Some(_) => "ready".to_string(),
                    None => "loading".to_string(),
                },
            );
            let Some(listing) = listing else {
                continue;
            };
            for model in &listing.models {
                let row = format!("{}\t{}", endpoint.name, model.id);
                facts.insert(row.clone(), model.clone());
                add(&mut rows, row);
            }
        }
        for (name, model) in waiting(&endpoints) {
            groups.insert(name.clone(), NEEDS_LOGIN.to_string());
            add(&mut rows, format!("{name}\t{model}"));
        }
        self.send(Msg::Models {
            request,
            recent,
            rows: rows.clone(),
            facts: facts.clone(),
            groups: groups.clone(),
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
        drop(crate::catalog::refresh_registry().await);
        let catalogs = join_all(endpoints.into_iter().map(|endpoint| async move {
            let covered = crate::catalog::plan_cost(&endpoint.name);
            let fetch = async {
                let client = Client::connect(&endpoint)?;
                let mut models = client.catalog(covered).await?;
                crate::catalog::enrich(&endpoint.name, endpoint.base_url.as_deref(), &mut models);
                join_all(models.iter_mut().map(|model| async {
                    if model.context.is_none() {
                        model.context = client.context(&model.id).await;
                    }
                }))
                .await;
                Ok::<_, crate::error::Error>(models)
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
                    for model in &models {
                        let row = format!("{name}\t{}", model.id);
                        facts.insert(row.clone(), model.clone());
                        add(&mut rows, row);
                    }
                    if let Err(error) = crate::catalog::remember(&name, models) {
                        errors.push(format!("{name}: cannot cache models ({error})"));
                    }
                    groups.insert(name, "ready".to_string());
                }
                Err(error) => {
                    groups.insert(name.clone(), "unreachable".to_string());
                    errors.push(format!("{name}: {error}"));
                }
            }
        }
        let tail = recent.min(rows.len());
        let mut best: BTreeMap<&str, u8> = BTreeMap::new();
        for row in &rows[tail..] {
            let name = row.split_once('\t').map_or("", |(name, _)| name);
            let rank = facts.get(row).map_or(2, |model| model.cost.rank());
            best.entry(name)
                .and_modify(|held| *held = (*held).min(rank))
                .or_insert(rank);
        }
        let order: BTreeMap<String, (u8, String)> = best
            .into_iter()
            .map(|(name, rank)| (name.to_string(), (rank, name.to_string())))
            .collect();
        rows[tail..].sort_by_cached_key(|row| {
            let name = row.split_once('\t').map_or("", |(name, _)| name);
            let group = order.get(name).cloned().unwrap_or((2, name.to_string()));
            let rank = facts.get(row).map_or(2, |model| model.cost.rank());
            (group, rank, row.clone())
        });
        self.send(Msg::Models {
            request,
            recent,
            rows,
            groups,
            facts,
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

fn walk(root: &Path, depth: Option<usize>) -> impl Iterator<Item = ignore::DirEntry> {
    WalkBuilder::new(root)
        .require_git(false)
        .max_depth(depth)
        .build()
        .filter_map(std::result::Result::ok)
}

fn watch(watcher: &mut impl Watcher, dirs: Vec<PathBuf>) -> notify::Result<()> {
    if dirs.is_empty() {
        return Ok(());
    }
    let mut paths = watcher.paths_mut();
    for entry in dirs.iter().flat_map(|dir| walk(dir, None)) {
        if entry.file_type().is_some_and(|kind| kind.is_dir()) {
            paths.add(entry.path(), RecursiveMode::NonRecursive)?;
        }
    }
    paths.commit()
}

fn files(workspace: &Path) -> Vec<String> {
    let Ok(root) = std::fs::canonicalize(workspace) else {
        return Vec::new();
    };
    walk(&root, None)
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
    let renamed = matches!(&event.kind, EventKind::Modify(ModifyKind::Name(_)));
    event.paths[event.paths.len().saturating_sub(1) * usize::from(renamed)..]
        .iter()
        .filter_map(|path| path.strip_prefix(root).ok().map(text))
        .filter(|path| !path.is_empty())
        .collect()
}

fn add(rows: &mut Vec<String>, row: String) {
    if !rows.contains(&row) {
        rows.push(row);
    }
}

fn text(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn unignored(root: &Path, mut paths: Vec<String>) -> Vec<String> {
    paths.sort();
    paths.dedup();
    let mut listed = std::collections::HashMap::new();
    paths.retain(|path| {
        let full = root.join(path);
        let dir = full.parent().unwrap_or(root).to_path_buf();
        let shown = listed.entry(dir).or_insert_with_key(|dir| {
            walk(dir, Some(1))
                .map(ignore::DirEntry::into_path)
                .collect::<Vec<_>>()
        });
        !path.split('/').any(|part| part.starts_with('.'))
            && (!full.exists() || shown.contains(&full))
    });
    paths
}
